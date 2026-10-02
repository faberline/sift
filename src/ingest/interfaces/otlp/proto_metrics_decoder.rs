//! OTLP protobuf metrics into metric events, with their temporality and
//! exemplars.

use anyhow::Result;
use transport_otlp::proto as wire;
use transport_otlp::proto::{metric, number_data_point};

use super::metric_normalization::{explicit_histogram, metric_event, MetricInput};
use crate::ingest::interfaces::otlp::otlp_codec::{item_error, OtlpItemError};
use crate::ingest::interfaces::otlp::otlp_proto_values::{
    proto_attributes, proto_resource, proto_scope, valid_proto_id,
};
use crate::{MetricExemplar, MetricTemporality, OperationalEventV2};

pub(super) fn decode_metrics_proto(
    request: wire::ExportMetricsServiceRequest,
    project: &str,
) -> Result<Vec<std::result::Result<OperationalEventV2, OtlpItemError>>> {
    let mut output = Vec::new();
    for resource_metrics in request.resource_metrics {
        let resource = proto_resource(resource_metrics.resource.as_ref());
        for scope_metrics in resource_metrics.scope_metrics {
            let scope = proto_scope(scope_metrics.scope.as_ref(), &scope_metrics.schema_url);
            for metric in scope_metrics.metrics {
                let name = metric.name.clone();
                let unit = (!metric.unit.is_empty()).then_some(metric.unit.clone());
                let Some(data) = metric.data else {
                    output.push(Err(item_error(
                        None,
                        format!("metric `{name}` has no data"),
                    )));
                    continue;
                };
                let (temporality, count) = match &data {
                    metric::Data::Sum(sum) => (
                        proto_temporality(sum.aggregation_temporality),
                        sum.data_points.len(),
                    ),
                    metric::Data::Histogram(histogram) => (
                        proto_temporality(histogram.aggregation_temporality),
                        histogram.data_points.len(),
                    ),
                    metric::Data::ExponentialHistogram(histogram) => (
                        proto_temporality(histogram.aggregation_temporality),
                        histogram.data_points.len(),
                    ),
                    _ => (Ok(MetricTemporality::Gauge), 0),
                };
                let temporality =
                    match temporality {
                        Ok(value) => value,
                        Err(error) => {
                            output.extend((0..count.max(1)).map(|_| {
                                Err(item_error(None, format!("metric `{name}`: {error}")))
                            }));
                            continue;
                        }
                    };
                let (points, temporality, monotonic): (
                    Vec<wire::NumberDataPoint>,
                    MetricTemporality,
                    bool,
                ) = match data {
                    metric::Data::Gauge(gauge) => {
                        (gauge.data_points, MetricTemporality::Gauge, false)
                    }
                    metric::Data::Sum(sum) => (sum.data_points, temporality, sum.is_monotonic),
                    metric::Data::Histogram(histogram) => {
                        for point in histogram.data_points {
                            let value = point.sum.unwrap_or(point.count as f64);
                            let result = metric_event(
                                project,
                                &resource,
                                scope.clone(),
                                MetricInput {
                                    name: name.clone(),
                                    unit: unit.clone(),
                                    value,
                                    temporality,
                                    monotonic: false,
                                    start: point.start_time_unix_nano,
                                    end: point.time_unix_nano,
                                    flags: point.flags,
                                    attributes: proto_attributes(&point.attributes),
                                    exemplars: proto_exemplars(&point.exemplars),
                                    histogram: Some(explicit_histogram(
                                        point.count,
                                        point.sum,
                                        point.explicit_bounds,
                                        point.bucket_counts,
                                        point.min,
                                        point.max,
                                    )),
                                },
                            );
                            output.push(result.map_err(|error| {
                                item_error(None, format!("metric `{name}`: {error}"))
                            }));
                        }
                        continue;
                    }
                    metric::Data::ExponentialHistogram(histogram) => {
                        for point in histogram.data_points {
                            let positive = point.positive.unwrap_or_default();
                            let negative = point.negative.unwrap_or_default();
                            let result = metric_event(project, &resource, scope.clone(), MetricInput {
                                name: name.clone(), unit: unit.clone(), value: point.sum.unwrap_or(point.count as f64),
                                temporality, monotonic: false,
                                start: point.start_time_unix_nano, end: point.time_unix_nano, flags: point.flags,
                                attributes: proto_attributes(&point.attributes), exemplars: proto_exemplars(&point.exemplars),
                                histogram: Some(crate::shared_kernel::projection_metric::MetricHistogramV1 {
                                    kind: crate::shared_kernel::projection_metric::HistogramKind::Exponential,
                                    count: point.count, sum: point.sum.unwrap_or(0.0), sum_present: point.sum.is_some(),
                                    scale: Some(point.scale), zero_count: point.zero_count, zero_threshold: point.zero_threshold,
                                    positive_offset: positive.offset, positive_bucket_counts: positive.bucket_counts,
                                    negative_offset: negative.offset, negative_bucket_counts: negative.bucket_counts,
                                    explicit_bounds: Vec::new(), bucket_counts: Vec::new(), min: point.min, max: point.max,
                                }),
                            });
                            output.push(result.map_err(|error| {
                                item_error(None, format!("metric `{name}`: {error}"))
                            }));
                        }
                        continue;
                    }
                    metric::Data::Summary(_) => {
                        output.push(Err(item_error(
                            None,
                            format!("metric `{name}` has unsupported data"),
                        )));
                        continue;
                    }
                };
                for point in points {
                    let Some(value) = point
                        .value
                        .and_then(proto_number)
                        .or_else(|| (point.flags & 1 != 0).then_some(0.0))
                    else {
                        output.push(Err(item_error(
                            None,
                            format!("metric `{name}` point has no value"),
                        )));
                        continue;
                    };
                    let result = metric_event(
                        project,
                        &resource,
                        scope.clone(),
                        MetricInput {
                            name: name.clone(),
                            unit: unit.clone(),
                            value,
                            temporality,
                            monotonic,
                            start: point.start_time_unix_nano,
                            end: point.time_unix_nano,
                            flags: point.flags,
                            attributes: proto_attributes(&point.attributes),
                            exemplars: proto_exemplars(&point.exemplars),
                            histogram: None,
                        },
                    );
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

fn proto_temporality(value: i32) -> Result<MetricTemporality> {
    match value {
        1 => Ok(MetricTemporality::Delta),
        2 => Ok(MetricTemporality::Cumulative),
        _ => anyhow::bail!("aggregationTemporality must be DELTA or CUMULATIVE"),
    }
}

fn proto_number(value: number_data_point::Value) -> Option<f64> {
    match value {
        number_data_point::Value::AsDouble(value) if value.is_finite() => Some(value),
        number_data_point::Value::AsInt(value) => Some(value as f64),
        _ => None,
    }
}

fn proto_exemplars(values: &[wire::Exemplar]) -> Vec<MetricExemplar> {
    values
        .iter()
        .filter_map(|value| {
            Some(MetricExemplar {
                value: value.value.and_then(|value| match value {
                    wire::exemplar::Value::AsDouble(value) if value.is_finite() => Some(value),
                    wire::exemplar::Value::AsInt(value) => Some(value as f64),
                    _ => None,
                })?,
                trace_id: valid_proto_id(&value.trace_id, 16)?,
                span_id: valid_proto_id(&value.span_id, 8)?,
            })
        })
        .collect()
}
