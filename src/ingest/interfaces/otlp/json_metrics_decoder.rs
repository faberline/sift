//! OTLP/JSON metrics into metric events, with their temporality and exemplars.

use anyhow::Result;
use serde_json::Value;

use super::metric_json_fields as fields;
use super::metric_normalization::{explicit_histogram, metric_event, MetricInput};
use crate::ingest::interfaces::otlp::otlp_codec::{item_error, OtlpItemError};
use crate::ingest::interfaces::otlp::otlp_json_attributes::{
    json_attributes, json_resource, json_scope,
};
use crate::ingest::interfaces::otlp::otlp_json_values::{array, id_string, string};
use crate::{MetricExemplar, MetricTemporality, OperationalEventV2};

pub(super) fn decode_metrics_json(
    root: &Value,
    project: &str,
) -> Result<Vec<std::result::Result<OperationalEventV2, OtlpItemError>>> {
    let mut output = Vec::new();
    for resource_metrics in array(root, "resourceMetrics", "resource_metrics") {
        let resource = json_resource(resource_metrics.get("resource"));
        for scope_metrics in array(resource_metrics, "scopeMetrics", "scope_metrics") {
            let scope = json_scope(
                scope_metrics.get("scope"),
                string(scope_metrics, "schemaUrl", "schema_url"),
            );
            for metric in array(scope_metrics, "metrics", "metrics") {
                let name = string(metric, "name", "name").unwrap_or("");
                let unit = string(metric, "unit", "unit").map(str::to_string);
                let exponential = metric
                    .get("exponentialHistogram")
                    .or_else(|| metric.get("exponential_histogram"));
                let (data, temporality, histogram) = if let Some(gauge) = metric.get("gauge") {
                    (gauge, Ok(MetricTemporality::Gauge), false)
                } else if let Some(sum) = metric.get("sum") {
                    (sum, json_temporality(sum), false)
                } else if let Some(histogram) = metric.get("histogram") {
                    (histogram, json_temporality(histogram), true)
                } else if let Some(histogram) = exponential {
                    (histogram, json_temporality(histogram), true)
                } else {
                    output.push(Err(item_error(
                        None,
                        format!("metric `{name}` has unsupported data"),
                    )));
                    continue;
                };
                for point in array(data, "dataPoints", "data_points") {
                    let result = (|| -> Result<OperationalEventV2> {
                        let temporality =
                            *temporality.as_ref().map_err(|e| anyhow::anyhow!("{e}"))?;
                        let flags =
                            u32::try_from(fields::unsigned(point, "flags", "flags")?.unwrap_or(0))?;
                        let hist = if exponential.is_some() {
                            Some(json_exponential_histogram(point)?)
                        } else if histogram {
                            Some(explicit_histogram(
                                fields::unsigned(point, "count", "count")?
                                    .ok_or_else(|| anyhow::anyhow!("histogram count is missing"))?,
                                fields::number(point, "sum", "sum")?,
                                fields::array(point, "explicitBounds", "explicit_bounds")?
                                    .iter()
                                    .map(|value| {
                                        value.as_f64().ok_or_else(|| {
                                            anyhow::anyhow!("invalid histogram bound")
                                        })
                                    })
                                    .collect::<Result<Vec<_>>>()?,
                                fields::array(point, "bucketCounts", "bucket_counts")?
                                    .iter()
                                    .map(|value| {
                                        value
                                            .as_u64()
                                            .or_else(|| value.as_str()?.parse().ok())
                                            .ok_or_else(|| {
                                                anyhow::anyhow!("invalid histogram bucket")
                                            })
                                    })
                                    .collect::<Result<Vec<_>>>()?,
                                fields::number(point, "min", "min")?,
                                fields::number(point, "max", "max")?,
                            ))
                        } else {
                            None
                        };
                        let value = fields::point_number(point)?
                            .or(fields::number(point, "sum", "sum")?)
                            .or_else(|| hist.as_ref().map(|h| h.count as f64))
                            .or_else(|| (flags & 1 != 0).then_some(0.0))
                            .ok_or_else(|| anyhow::anyhow!("metric point has no numeric value"))?;
                        metric_event(
                            project,
                            &resource,
                            scope.clone(),
                            MetricInput {
                                name: name.into(),
                                unit: unit.clone(),
                                value,
                                temporality,
                                monotonic: fields::boolean(data, "isMonotonic", "is_monotonic")?
                                    .unwrap_or(false),
                                start: fields::unsigned(
                                    point,
                                    "startTimeUnixNano",
                                    "start_time_unix_nano",
                                )?
                                .unwrap_or(0),
                                end: fields::unsigned(point, "timeUnixNano", "time_unix_nano")?
                                    .unwrap_or(0),
                                flags,
                                attributes: json_attributes(point.get("attributes")),
                                exemplars: json_exemplars(point.get("exemplars")),
                                histogram: hist,
                            },
                        )
                    })();
                    output
                        .push(result.map_err(|error| {
                            item_error(None, format!("metric `{name}`: {error}"))
                        }));
                }
            }
        }
    }
    Ok(output)
}

fn json_exponential_histogram(
    point: &Value,
) -> Result<crate::shared_kernel::projection_metric::MetricHistogramV1> {
    use crate::shared_kernel::projection_metric::{HistogramKind, MetricHistogramV1};
    let bucket = |name: &str| -> Result<(i32, Vec<u64>)> {
        let value = point.get(name).unwrap_or(&Value::Null);
        let offset = fields::signed(value, "offset", "offset")?.unwrap_or(0);
        let counts = fields::array(value, "bucketCounts", "bucket_counts")?
            .iter()
            .map(|v| {
                v.as_u64()
                    .or_else(|| v.as_str()?.parse().ok())
                    .ok_or_else(|| anyhow::anyhow!("invalid exponential bucket count"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((i32::try_from(offset)?, counts))
    };
    let (positive_offset, positive_bucket_counts) = bucket("positive")?;
    let (negative_offset, negative_bucket_counts) = bucket("negative")?;
    let scale = fields::signed(point, "scale", "scale")?.unwrap_or(0);
    let sum = fields::number(point, "sum", "sum")?;
    Ok(MetricHistogramV1 {
        kind: HistogramKind::Exponential,
        count: fields::unsigned(point, "count", "count")?
            .ok_or_else(|| anyhow::anyhow!("exponential histogram count is missing"))?,
        sum: sum.unwrap_or(0.0),
        sum_present: sum.is_some(),
        scale: Some(i32::try_from(scale)?),
        zero_count: fields::unsigned(point, "zeroCount", "zero_count")?.unwrap_or(0),
        zero_threshold: fields::number(point, "zeroThreshold", "zero_threshold")?.unwrap_or(0.0),
        positive_offset,
        positive_bucket_counts,
        negative_offset,
        negative_bucket_counts,
        explicit_bounds: Vec::new(),
        bucket_counts: Vec::new(),
        min: fields::number(point, "min", "min")?,
        max: fields::number(point, "max", "max")?,
    })
}

fn json_temporality(value: &Value) -> Result<MetricTemporality> {
    let value = fields::field(value, "aggregationTemporality", "aggregation_temporality");
    match value.and_then(Value::as_str) {
        Some("AGGREGATION_TEMPORALITY_DELTA") => return Ok(MetricTemporality::Delta),
        Some("AGGREGATION_TEMPORALITY_CUMULATIVE") => return Ok(MetricTemporality::Cumulative),
        _ => {}
    }
    match value.and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok())) {
        Some(1) => Ok(MetricTemporality::Delta),
        Some(2) => Ok(MetricTemporality::Cumulative),
        _ => anyhow::bail!("aggregationTemporality must be DELTA or CUMULATIVE"),
    }
}

fn json_exemplars(value: Option<&Value>) -> Vec<MetricExemplar> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|exemplar| {
            Some(MetricExemplar {
                value: fields::point_number(exemplar).ok().flatten()?,
                trace_id: id_string(exemplar.get("traceId").or_else(|| exemplar.get("trace_id")))?,
                span_id: id_string(exemplar.get("spanId").or_else(|| exemplar.get("span_id")))?,
            })
        })
        .collect()
}
