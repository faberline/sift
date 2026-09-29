//! The operational event an OTLP item becomes: its project, environment, times,
//! resource and the sift.* attributes that override its ids.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::Value;
use transport_otlp::OtlpSignal;

use crate::{AttributeValue, OperationalEventV2, SignalKind};

pub(super) fn apply_common_attributes(event: &mut OperationalEventV2) {
    if let Some(AttributeValue::String(event_id)) = event.attributes.get("sift.event_id") {
        if !event_id.trim().is_empty() {
            event.event_id.clone_from(event_id);
        }
    }
    if let Some(AttributeValue::String(request_id)) = event.attributes.get("sift.request_id") {
        event.request_id = Some(request_id.clone());
    }
    if let Some(AttributeValue::String(session_id)) = event.attributes.get("sift.session_id") {
        event.session_id = Some(session_id.clone());
    }
}

pub(super) fn base_event(
    signal: OtlpSignal,
    project_hint: &str,
    resource: &BTreeMap<String, String>,
    event_id: String,
    occurred_nanos: u64,
    observed_nanos: u64,
    payload: Value,
) -> OperationalEventV2 {
    // The authenticated request project is the tenant boundary. Cloud account
    // and infrastructure project attributes remain queryable resource data;
    // they must never override the admitted Sift project.
    let project = project_hint;
    let environment = resource
        .get("deployment.environment.name")
        .or_else(|| resource.get("environment"))
        .map(String::as_str)
        .unwrap_or("default");
    let occurred_at = nanos_to_rfc3339(occurred_nanos);
    let observed_at = nanos_to_rfc3339(if observed_nanos == 0 {
        occurred_nanos
    } else {
        observed_nanos
    });
    let mut event = OperationalEventV2::for_project(
        project,
        environment,
        event_id,
        signal_kind(signal),
        payload,
    );
    event.occurred_at = occurred_at;
    event.observed_at = observed_at;
    event.resource = resource.clone();
    if event.resource.is_empty() {
        event
            .resource
            .insert("service.name".into(), "unknown".into());
    }
    event
}

fn signal_kind(signal: OtlpSignal) -> SignalKind {
    match signal {
        OtlpSignal::Logs => SignalKind::Log,
        OtlpSignal::Metrics => SignalKind::Metric,
        OtlpSignal::Traces => SignalKind::Span,
    }
}

fn nanos_to_rfc3339(value: u64) -> String {
    if value == 0 {
        return Utc::now().to_rfc3339();
    }
    DateTime::<Utc>::from_timestamp(
        (value / 1_000_000_000) as i64,
        (value % 1_000_000_000) as u32,
    )
    .unwrap_or_else(Utc::now)
    .to_rfc3339()
}
