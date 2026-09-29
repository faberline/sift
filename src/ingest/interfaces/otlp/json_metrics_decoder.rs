//! OTLP/JSON metrics into metric events, with their temporality and exemplars.

use anyhow::Result;
use serde_json::{json, Value};
use transport_otlp::OtlpSignal;

use crate::ingest::domain::otlp_event_id::stable_id;
use crate::ingest::interfaces::otlp::otlp_codec::{item_error, OtlpItemError};
use crate::ingest::interfaces::otlp::otlp_event_builder::{apply_common_attributes, base_event};
use crate::ingest::interfaces::otlp::otlp_json_attributes::{
    json_attributes, json_resource, json_scope,
};
use crate::ingest::interfaces::otlp::otlp_json_values::{array, id_string, nanos, string};
use crate::{MetricExemplar, MetricPoint, MetricTemporality, OperationalEventV2};

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
                let (data, temporality) = if let Some(gauge) = metric.get("gauge") {
                    (gauge, MetricTemporality::Gauge)
                } else if let Some(sum) = metric.get("sum") {
                    (sum, json_temporality(sum))
                } else if let Some(histogram) = metric.get("histogram") {
                    (histogram, json_temporality(histogram))
                } else {
                    output.push(Err(item_error(
                        None,
                        format!("metric `{name}` has unsupported data"),
                    )));
                    continue;
                };
                for point in array(data, "dataPoints", "data_points") {
                    let value =
                        json_number(point).or_else(|| point.get("sum").and_then(Value::as_f64));
                    let Some(value) = value else {
                        output.push(Err(item_error(
                            None,
                            format!("metric `{name}` point has no numeric value"),
                        )));
                        continue;
                    };
                    let time = nanos(point, "timeUnixNano", "time_unix_nano");
                    let identity = format!(
                        "{name}:{time}:{}",
                        serde_json::to_string(point).unwrap_or_default()
                    );
                    let mut event = base_event(
                        OtlpSignal::Metrics,
                        project,
                        &resource,
                        stable_id("metric", project, &identity),
                        time,
                        time,
                        json!({"metric": metric, "point": point}),
                    );
                    event.instrumentation_scope = scope.clone();
                    event.attributes = json_attributes(point.get("attributes"));
                    apply_common_attributes(&mut event);
                    event.metric = Some(MetricPoint {
                        name: name.to_string(),
                        value,
                        stale: false,
                        unit: unit.clone(),
                        temporality,
                        exemplars: json_exemplars(point.get("exemplars")),
                    });
                    output.push(Ok(event));
                }
            }
        }
    }
    Ok(output)
}

fn json_number(point: &Value) -> Option<f64> {
    point
        .get("asDouble")
        .or_else(|| point.get("as_double"))
        .and_then(|value| value.as_f64().or_else(|| value.as_str()?.parse().ok()))
        .or_else(|| {
            point
                .get("asInt")
                .or_else(|| point.get("as_int"))
                .and_then(|value| {
                    value
                        .as_i64()
                        .map(|value| value as f64)
                        .or_else(|| value.as_str()?.parse().ok())
                })
        })
}

fn json_temporality(value: &Value) -> MetricTemporality {
    match value
        .get("aggregationTemporality")
        .or_else(|| value.get("aggregation_temporality"))
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
    {
        Some(1) => MetricTemporality::Delta,
        _ => MetricTemporality::Cumulative,
    }
}

fn json_exemplars(value: Option<&Value>) -> Vec<MetricExemplar> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|exemplar| {
            Some(MetricExemplar {
                value: json_number(exemplar)?,
                trace_id: id_string(exemplar.get("traceId").or_else(|| exemplar.get("trace_id")))?,
                span_id: id_string(exemplar.get("spanId").or_else(|| exemplar.get("span_id")))?,
            })
        })
        .collect()
}
