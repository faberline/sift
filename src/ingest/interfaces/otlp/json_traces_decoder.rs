//! OTLP/JSON traces into span events.

use anyhow::Result;
use serde_json::{json, Value};
use transport_otlp::OtlpSignal;

use crate::ingest::domain::otlp_event_id::stable_id;
use crate::ingest::interfaces::otlp::otlp_codec::{item_error, OtlpItemError};
use crate::ingest::interfaces::otlp::otlp_event_builder::{apply_common_attributes, base_event};
use crate::ingest::interfaces::otlp::otlp_json_attributes::{
    json_attributes, json_resource, json_scope,
};
use crate::ingest::interfaces::otlp::otlp_json_values::{
    array, id_string, nanos, scalar_value_string, string,
};
use crate::OperationalEventV2;

pub(super) fn decode_traces_json(
    root: &Value,
    project: &str,
) -> Result<Vec<std::result::Result<OperationalEventV2, OtlpItemError>>> {
    let mut output = Vec::new();
    for resource_spans in array(root, "resourceSpans", "resource_spans") {
        let resource = json_resource(resource_spans.get("resource"));
        for scope_spans in array(resource_spans, "scopeSpans", "scope_spans") {
            let scope = json_scope(
                scope_spans.get("scope"),
                string(scope_spans, "schemaUrl", "schema_url"),
            );
            for span in array(scope_spans, "spans", "spans") {
                let trace_id = id_string(span.get("traceId").or_else(|| span.get("trace_id")));
                let span_id = id_string(span.get("spanId").or_else(|| span.get("span_id")));
                let name = string(span, "name", "name").unwrap_or("");
                if trace_id.is_none() || span_id.is_none() || name.is_empty() {
                    output.push(Err(item_error(
                        span_id,
                        "span requires traceId, spanId, and name",
                    )));
                    continue;
                }
                let start = nanos(span, "startTimeUnixNano", "start_time_unix_nano");
                let end = nanos(span, "endTimeUnixNano", "end_time_unix_nano");
                let events = array(span, "events", "events")
                    .iter()
                    .map(|event| {
                        json!({
                            "name": string(event, "name", "name").unwrap_or(""),
                            "time_unix_nano": nanos(event, "timeUnixNano", "time_unix_nano"),
                            "attributes": json_attributes(event.get("attributes")),
                        })
                    })
                    .collect::<Vec<_>>();
                let links = array(span, "links", "links")
                    .iter()
                    .filter_map(|link| {
                        Some(json!({
                            "trace_id": id_string(link.get("traceId").or_else(|| link.get("trace_id")))?,
                            "span_id": id_string(link.get("spanId").or_else(|| link.get("span_id")))?,
                            "attributes": json_attributes(link.get("attributes")),
                        }))
                    })
                    .collect::<Vec<_>>();
                let status = span.get("status").map(|status| {
                    json!({
                        "code": status.get("code").cloned(),
                        "message": string(status, "message", "message"),
                    })
                });
                let mut event = base_event(
                    OtlpSignal::Traces,
                    project,
                    &resource,
                    stable_id(
                        "span",
                        project,
                        &format!(
                            "{}:{}",
                            trace_id.as_deref().unwrap_or(""),
                            span_id.as_deref().unwrap_or(name)
                        ),
                    ),
                    start,
                    end,
                    json!({
                        "name": name,
                        "kind": scalar_value_string(span.get("kind")),
                        "parent_span_id": id_string(span.get("parentSpanId").or_else(|| span.get("parent_span_id"))),
                        "start_time_unix_nano": start,
                        "end_time_unix_nano": end,
                        "status": status,
                        "events": events,
                        "links": links,
                    }),
                );
                event.instrumentation_scope = scope.clone();
                event.attributes = json_attributes(span.get("attributes"));
                apply_common_attributes(&mut event);
                event.trace_id = trace_id;
                event.span_id = span_id;
                output.push(Ok(event));
            }
        }
    }
    Ok(output)
}
