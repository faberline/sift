//! Turning a stored log event into the record the logging projection keeps.

use crate::projection::domain::log::LogRecordV1;
use crate::shared_kernel::stored_event::StoredEvent;
use crate::AttributeValue;

pub(in crate::projection) fn normalize(stored: &StoredEvent) -> LogRecordV1 {
    let event = &stored.event;
    let json_payload = event
        .payload
        .get("jsonPayload")
        .cloned()
        .unwrap_or_else(|| event.payload.clone());
    let body_text = event
        .attributes
        .get("otel.log.body")
        .and_then(AttributeValue::as_str)
        .map(str::to_owned)
        .or_else(|| {
            json_payload
                .get("message")
                .or_else(|| json_payload.get("body"))
                .or_else(|| event.payload.get("body"))
                .or_else(|| event.payload.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| json_payload.to_string());
    LogRecordV1 {
        cursor: stored.cursor,
        event_id: event.event_id.clone(),
        project: event.project.clone(),
        environment: event.environment.clone(),
        occurred_at: event.occurred_at.clone(),
        observed_at: event.observed_at.clone(),
        severity: event.severity.clone(),
        body_text,
        json_payload,
        resource: event.resource.clone(),
        attributes: event.attributes.clone(),
        trace_id: event.trace_id.clone(),
        span_id: event.span_id.clone(),
        request_id: event.request_id.clone(),
        session_id: event.session_id.clone(),
        coexistence_key: format!("{}:{}", event.project, event.event_id),
    }
}
