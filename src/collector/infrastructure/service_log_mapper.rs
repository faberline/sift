//! Decode and strictly validate ServiceLogEventV1, convert bounded primitive
//! attributes, preserve payload, validate correlation, and derive stable ids.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde_json::Value;
use service_observability::{StructuredServiceLogV1, SERVICE_LOG_SCHEMA_V1};

use crate::collector::domain::record::RecordEnrichment;
use crate::collector::domain::service_log::{deterministic_event_id, trim_line_ending, validate};
use crate::{AttributeValue, InstrumentationScope, OperationalEventV2, SignalKind};

pub fn decode_service_log(
    raw_line: &[u8],
    source_id: &str,
    offset: u64,
    project: &str,
    environment: &str,
) -> Result<OperationalEventV2> {
    decode_service_log_enriched(
        raw_line,
        source_id,
        offset,
        project,
        environment,
        &RecordEnrichment::default(),
    )
}

pub(crate) fn decode_service_log_enriched(
    raw_line: &[u8],
    source_id: &str,
    offset: u64,
    project: &str,
    environment: &str,
    enrichment: &RecordEnrichment,
) -> Result<OperationalEventV2> {
    let json = trim_line_ending(raw_line);
    let log: StructuredServiceLogV1 =
        serde_json::from_slice(json).context("decode axiom service log JSON")?;
    validate(&log)?;

    let event_id = deterministic_event_id(source_id, offset, raw_line);
    let payload = serde_json::to_value(&log)?;
    let mut event =
        OperationalEventV2::for_project(project, environment, event_id, SignalKind::Log, payload);
    event.occurred_at = log.timestamp.clone();
    event.observed_at = Utc::now().to_rfc3339();
    event
        .resource
        .insert("service.name".to_string(), log.service.name.clone());
    event
        .resource
        .insert("service.version".to_string(), log.service.version.clone());
    event
        .resource
        .insert("collector.source_id".to_string(), source_id.to_string());
    event.instrumentation_scope = Some(InstrumentationScope {
        name: log.service.name.clone(),
        version: Some(log.service.version.clone()),
        attributes: BTreeMap::new(),
        schema_url: Some(SERVICE_LOG_SCHEMA_V1.to_string()),
    });
    for (key, value) in &log.attributes {
        if let Some(value) = attribute_value(value)? {
            event.attributes.insert(key.clone(), value);
        }
    }
    event.attributes.insert(
        "event.name".to_string(),
        AttributeValue::String(log.event.clone()),
    );
    event.attributes.insert(
        "collector.source_id".to_string(),
        AttributeValue::String(source_id.to_string()),
    );
    event.attributes.insert(
        "collector.offset".to_string(),
        i64::try_from(offset)
            .map(AttributeValue::Int)
            .unwrap_or_else(|_| AttributeValue::String(offset.to_string())),
    );
    if let Some(parent_span_id) = log.parent_span_id.as_ref() {
        event.attributes.insert(
            "parent_span_id".to_string(),
            AttributeValue::String(parent_span_id.clone()),
        );
    }
    if let Some(trace_flags) = log.trace_flags.as_ref() {
        event.attributes.insert(
            "trace.flags".to_string(),
            AttributeValue::String(trace_flags.clone()),
        );
    }
    event.trace_id = log.trace_id;
    event.span_id = log.span_id;
    event.request_id = log.request_id;
    event.severity = Some(log.severity);
    event.resource.extend(enrichment.resource.clone());
    event.attributes.extend(enrichment.attributes.clone());
    if enrichment.cloud_logging_coexistence {
        let resource_type = event
            .resource
            .get("gcp.resource.type")
            .context("CRI enrichment requires gcp.resource.type")?;
        event.event_id = crate::shared_kernel::cloud_logging_event_id::stable_id(
            project,
            resource_type,
            &event.occurred_at,
            &event.resource,
            &event.payload,
        );
    }
    event.validate()?;
    Ok(event)
}

fn attribute_value(value: &Value) -> Result<Option<AttributeValue>> {
    Ok(match value {
        Value::Null => None,
        Value::String(value) => Some(AttributeValue::String(value.clone())),
        Value::Bool(value) => Some(AttributeValue::Bool(*value)),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Some(AttributeValue::Int(value))
            } else {
                let value = value.as_f64().context("JSON number is not representable")?;
                if !value.is_finite() {
                    bail!("JSON number must be finite");
                }
                Some(AttributeValue::Double(value))
            }
        }
        Value::Array(_) | Value::Object(_) => {
            bail!("collector attributes must be primitive JSON values")
        }
    })
}

#[cfg(test)]
mod tests {
    use service_observability::ServiceLogIdentityV1;

    use super::*;

    fn fixture() -> StructuredServiceLogV1 {
        StructuredServiceLogV1 {
            schema: SERVICE_LOG_SCHEMA_V1.to_string(),
            timestamp: "2026-07-17T10:00:00Z".to_string(),
            severity: "INFO".to_string(),
            service: ServiceLogIdentityV1 {
                name: "lumen".to_string(),
                version: "0.4.21".to_string(),
            },
            event: "collection_create_or_extend".to_string(),
            message: "collection created".to_string(),
            trace_id: Some("0af7651916cd43dd8448eb211c80319c".to_string()),
            span_id: Some("b7ad6b7169203331".to_string()),
            parent_span_id: Some("00f067aa0ba902b7".to_string()),
            trace_flags: Some("01".to_string()),
            request_id: Some("request-42".to_string()),
            attributes: BTreeMap::from([
                (
                    "collection_id".to_string(),
                    Value::String("docs".to_string()),
                ),
                ("fields".to_string(), Value::Number(2.into())),
            ]),
        }
    }

    #[test]
    fn maps_shared_log_to_canonical_event_with_stable_id() {
        let raw = serde_json::to_vec(&fixture()).unwrap();
        let first = decode_service_log(&raw, "fixture", 10, "local", "test").unwrap();
        let again = decode_service_log(&raw, "fixture", 10, "local", "test").unwrap();
        let shifted = decode_service_log(&raw, "fixture", 11, "local", "test").unwrap();

        assert_eq!(first.event_id, again.event_id);
        assert_ne!(first.event_id, shifted.event_id);
        assert_eq!(first.project, "local");
        assert_eq!(first.resource["service.name"], "lumen");
        assert_eq!(
            first.trace_id.as_deref(),
            Some("0af7651916cd43dd8448eb211c80319c")
        );
        assert_eq!(
            first.attributes["parent_span_id"].as_str(),
            Some("00f067aa0ba902b7")
        );
        assert_eq!(first.payload["message"], "collection created");
    }

    #[test]
    fn rejects_wrong_schema_invalid_ids_and_nested_attributes() {
        let mut log = fixture();
        log.schema = "other.v1".to_string();
        assert!(decode_service_log(
            &serde_json::to_vec(&log).unwrap(),
            "fixture",
            0,
            "local",
            "test"
        )
        .is_err());

        let mut log = fixture();
        log.trace_id = Some("ABCDEFABCDEFABCDEFABCDEFABCDEFAB".to_string());
        assert!(decode_service_log(
            &serde_json::to_vec(&log).unwrap(),
            "fixture",
            0,
            "local",
            "test"
        )
        .is_err());

        let mut log = fixture();
        log.attributes
            .insert("nested".to_string(), serde_json::json!({"no": "objects"}));
        assert!(decode_service_log(
            &serde_json::to_vec(&log).unwrap(),
            "fixture",
            0,
            "local",
            "test"
        )
        .is_err());
    }
}
