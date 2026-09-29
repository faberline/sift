//! OTLP protobuf metrics into metric events, with their temporality and
//! exemplars.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::{json, Value};
use transport_otlp::proto::{metric, number_data_point};
use transport_otlp::{proto as wire, OtlpSignal};

use crate::ingest::domain::otlp_event_id::stable_id;
use crate::ingest::interfaces::otlp::otlp_codec::{item_error, OtlpItemError};
use crate::ingest::interfaces::otlp::otlp_event_builder::{apply_common_attributes, base_event};
use crate::ingest::interfaces::otlp::otlp_proto_values::{
    proto_attributes, proto_resource, proto_scope, valid_proto_id,
};
use crate::{
    AttributeValue, InstrumentationScope, MetricExemplar, MetricPoint, MetricTemporality,
    OperationalEventV2,
};

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
                let (points, temporality): (Vec<wire::NumberDataPoint>, MetricTemporality) =
                    match data {
                        metric::Data::Gauge(gauge) => (gauge.data_points, MetricTemporality::Gauge),
                        metric::Data::Sum(sum) => (
                            sum.data_points,
                            proto_temporality(sum.aggregation_temporality),
                        ),
                        metric::Data::Histogram(histogram) => {
                            for point in histogram.data_points {
                                let value = point.sum.unwrap_or(point.count as f64);
                                output.push(Ok(proto_metric_event(
                                project,
                                &resource,
                                scope.clone(),
                                &name,
                                unit.clone(),
                                value,
                                proto_temporality(histogram.aggregation_temporality),
                                point.time_unix_nano,
                                proto_attributes(&point.attributes),
                                proto_exemplars(&point.exemplars),
                                json!({"count": point.count, "sum": point.sum, "bucketCounts": point.bucket_counts, "explicitBounds": point.explicit_bounds}),
                            )));
                            }
                            continue;
                        }
                        metric::Data::ExponentialHistogram(_) | metric::Data::Summary(_) => {
                            output.push(Err(item_error(
                                None,
                                format!("metric `{name}` has unsupported data"),
                            )));
                            continue;
                        }
                    };
                for point in points {
                    let Some(value) = point.value.and_then(proto_number) else {
                        output.push(Err(item_error(
                            None,
                            format!("metric `{name}` point has no value"),
                        )));
                        continue;
                    };
                    output.push(Ok(proto_metric_event(
                        project,
                        &resource,
                        scope.clone(),
                        &name,
                        unit.clone(),
                        value,
                        temporality,
                        point.time_unix_nano,
                        proto_attributes(&point.attributes),
                        proto_exemplars(&point.exemplars),
                        json!({"startTimeUnixNano": point.start_time_unix_nano, "flags": point.flags}),
                    )));
                }
            }
        }
    }
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn proto_metric_event(
    project: &str,
    resource: &BTreeMap<String, String>,
    scope: Option<InstrumentationScope>,
    name: &str,
    unit: Option<String>,
    value: f64,
    temporality: MetricTemporality,
    time: u64,
    attributes: BTreeMap<String, AttributeValue>,
    exemplars: Vec<MetricExemplar>,
    payload: Value,
) -> OperationalEventV2 {
    let identity = format!(
        "{name}:{time}:{value}:{}:{}",
        serde_json::to_string(&attributes).unwrap_or_default(),
        serde_json::to_string(&payload).unwrap_or_default()
    );
    let mut event = base_event(
        OtlpSignal::Metrics,
        project,
        resource,
        stable_id("metric", project, &identity),
        time,
        time,
        payload,
    );
    event.instrumentation_scope = scope;
    event.attributes = attributes;
    apply_common_attributes(&mut event);
    event.metric = Some(MetricPoint {
        name: name.to_string(),
        value,
        stale: false,
        unit,
        temporality,
        exemplars,
    });
    event
}

fn proto_temporality(value: i32) -> MetricTemporality {
    if value == 1 {
        MetricTemporality::Delta
    } else {
        MetricTemporality::Cumulative
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
                value: value.value.clone().and_then(|value| match value {
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
