//! OTLP protobuf traces into span events.

use anyhow::Result;
use serde_json::json;
use transport_otlp::{proto as wire, OtlpSignal};

use crate::ingest::domain::otlp_event_id::stable_id;
use crate::ingest::interfaces::otlp::otlp_codec::{item_error, OtlpItemError};
use crate::ingest::interfaces::otlp::otlp_event_builder::{apply_common_attributes, base_event};
use crate::ingest::interfaces::otlp::otlp_proto_values::{
    proto_attributes, proto_resource, proto_scope, valid_proto_id,
};
use crate::OperationalEventV2;

pub(super) fn decode_traces_proto(
    request: wire::ExportTraceServiceRequest,
    project: &str,
) -> Result<Vec<std::result::Result<OperationalEventV2, OtlpItemError>>> {
    let mut output = Vec::new();
    for resource_spans in request.resource_spans {
        let resource = proto_resource(resource_spans.resource.as_ref());
        for scope_spans in resource_spans.scope_spans {
            let scope = proto_scope(scope_spans.scope.as_ref(), &scope_spans.schema_url);
            for span in scope_spans.spans {
                let trace_id = valid_proto_id(&span.trace_id, 16);
                let span_id = valid_proto_id(&span.span_id, 8);
                if trace_id.is_none() || span_id.is_none() || span.name.is_empty() {
                    output.push(Err(item_error(
                        span_id,
                        "span requires valid trace_id, span_id, and name",
                    )));
                    continue;
                }
                let events = span
                    .events
                    .iter()
                    .map(|event| {
                        json!({
                            "name": event.name,
                            "time_unix_nano": event.time_unix_nano,
                            "attributes": proto_attributes(&event.attributes),
                        })
                    })
                    .collect::<Vec<_>>();
                let links = span
                    .links
                    .iter()
                    .filter_map(|link| {
                        Some(json!({
                            "trace_id": valid_proto_id(&link.trace_id, 16)?,
                            "span_id": valid_proto_id(&link.span_id, 8)?,
                            "attributes": proto_attributes(&link.attributes),
                        }))
                    })
                    .collect::<Vec<_>>();
                let kind =
                    opentelemetry_proto::tonic::trace::v1::span::SpanKind::try_from(span.kind)
                        .ok()
                        .map(|kind| kind.as_str_name().to_string());
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
                            span_id.as_deref().unwrap_or(&span.name)
                        ),
                    ),
                    span.start_time_unix_nano,
                    span.end_time_unix_nano,
                    json!({
                        "name": span.name,
                        "kind": kind,
                        "parent_span_id": valid_proto_id(&span.parent_span_id, 8),
                        "start_time_unix_nano": span.start_time_unix_nano,
                        "end_time_unix_nano": span.end_time_unix_nano,
                        "events": events,
                        "links": links,
                        "status": span.status.as_ref().map(|status| json!({"code": status.code, "message": status.message})),
                    }),
                );
                event.instrumentation_scope = scope.clone();
                event.attributes = proto_attributes(&span.attributes);
                apply_common_attributes(&mut event);
                event.trace_id = trace_id;
                event.span_id = span_id;
                output.push(Ok(event));
            }
        }
    }
    Ok(output)
}
