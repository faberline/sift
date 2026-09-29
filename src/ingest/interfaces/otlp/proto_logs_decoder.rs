//! OTLP protobuf logs into log events.

use anyhow::Result;
use prost14::Message;
use serde_json::json;
use transport_otlp::{proto as wire, OtlpSignal};

use crate::ingest::domain::otlp_event_id::stable_id;
use crate::ingest::interfaces::otlp::otlp_codec::{item_error, OtlpItemError};
use crate::ingest::interfaces::otlp::otlp_event_builder::{apply_common_attributes, base_event};
use crate::ingest::interfaces::otlp::otlp_proto_values::{
    proto_any_json, proto_attributes, proto_resource, proto_scope, valid_proto_id,
};
use crate::OperationalEventV2;

pub(super) fn decode_logs_proto(
    request: wire::ExportLogsServiceRequest,
    project: &str,
) -> Result<Vec<std::result::Result<OperationalEventV2, OtlpItemError>>> {
    let mut output = Vec::new();
    for resource_logs in request.resource_logs {
        let resource = proto_resource(resource_logs.resource.as_ref());
        for scope_logs in resource_logs.scope_logs {
            let scope = proto_scope(scope_logs.scope.as_ref(), &scope_logs.schema_url);
            for record in scope_logs.log_records {
                if record.body.is_none() {
                    output.push(Err(item_error(None, "log record body is required")));
                    continue;
                }
                let identity = format!(
                    "{}:{}:{}:{}",
                    hex::encode(&record.trace_id),
                    hex::encode(&record.span_id),
                    record.time_unix_nano,
                    hex::encode(record.encode_to_vec())
                );
                let mut event = base_event(
                    OtlpSignal::Logs,
                    project,
                    &resource,
                    stable_id("log", project, &identity),
                    record.time_unix_nano,
                    record.observed_time_unix_nano,
                    json!({
                        "body": record.body.as_ref().map(proto_any_json),
                        "severityNumber": record.severity_number,
                        "severityText": record.severity_text,
                        "flags": record.flags,
                        "droppedAttributesCount": record.dropped_attributes_count,
                    }),
                );
                event.instrumentation_scope = scope.clone();
                event.attributes = proto_attributes(&record.attributes);
                apply_common_attributes(&mut event);
                event.trace_id = valid_proto_id(&record.trace_id, 16);
                event.span_id = valid_proto_id(&record.span_id, 8);
                event.severity = (!record.severity_text.is_empty()).then_some(record.severity_text);
                output.push(Ok(event));
            }
        }
    }
    Ok(output)
}
