//! OTLP/JSON logs into log events.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::Value;
use transport_otlp::OtlpSignal;

use crate::ingest::domain::otlp_event_id::stable_id;
use crate::ingest::interfaces::otlp::otlp_codec::{item_error, OtlpItemError};
use crate::ingest::interfaces::otlp::otlp_event_builder::{apply_common_attributes, base_event};
use crate::ingest::interfaces::otlp::otlp_json_attributes::{
    json_any_value, json_attribute_value, json_attributes, json_resource, json_scope,
};
use crate::ingest::interfaces::otlp::otlp_json_values::{array, id_string, nanos, string};
use crate::{InstrumentationScope, OperationalEventV2};

pub(super) fn decode_logs_json(
    root: &Value,
    project: &str,
) -> Result<Vec<std::result::Result<OperationalEventV2, OtlpItemError>>> {
    let mut output = Vec::new();
    for resource_logs in array(root, "resourceLogs", "resource_logs") {
        let resource = json_resource(resource_logs.get("resource"));
        for scope_logs in array(resource_logs, "scopeLogs", "scope_logs") {
            let scope = json_scope(
                scope_logs.get("scope"),
                string(scope_logs, "schemaUrl", "schema_url"),
            );
            for record in array(scope_logs, "logRecords", "log_records") {
                output.push(json_log_event(record, project, &resource, scope.clone()));
            }
        }
    }
    Ok(output)
}

fn json_log_event(
    record: &Value,
    project: &str,
    resource: &BTreeMap<String, String>,
    scope: Option<InstrumentationScope>,
) -> std::result::Result<OperationalEventV2, OtlpItemError> {
    let body = record
        .get("body")
        .filter(|value| !value.is_null())
        .and_then(json_any_value)
        .filter(|value| !value.is_null());
    if body.is_none() {
        return Err(item_error(None, "log record body is required"));
    }
    let occurred_nanos = nanos(record, "timeUnixNano", "time_unix_nano");
    let observed_nanos = nanos(record, "observedTimeUnixNano", "observed_time_unix_nano");
    let trace_id = id_string(record.get("traceId").or_else(|| record.get("trace_id")));
    let span_id = id_string(record.get("spanId").or_else(|| record.get("span_id")));
    let identity = format!(
        "{}:{}:{}:{}",
        trace_id.as_deref().unwrap_or(""),
        span_id.as_deref().unwrap_or(""),
        occurred_nanos,
        serde_json::to_string(record).unwrap_or_default()
    );
    let mut event = base_event(
        OtlpSignal::Logs,
        project,
        resource,
        stable_id("log", project, &identity),
        occurred_nanos,
        observed_nanos,
        record.clone(),
    );
    event.instrumentation_scope = scope;
    event.attributes = json_attributes(record.get("attributes"));
    apply_common_attributes(&mut event);
    event.trace_id = trace_id;
    event.span_id = span_id;
    event.severity = string(record, "severityText", "severity_text").map(str::to_string);
    if let Some(body) = record.get("body").and_then(json_attribute_value) {
        event.attributes.insert("otel.log.body".into(), body);
    }
    Ok(event)
}
