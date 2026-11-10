//! JSON and text logs with the original record and source identity intact.
use super::service_log_mapper::decode_service_log_enriched;
use crate::collector::domain::log_format::LogFormat;
use crate::collector::domain::quarantine::QuarantineEntry;
use crate::collector::domain::record::RawRecord;
use crate::collector::domain::record_decoder::RecordDecoder;
use crate::collector::domain::service_log::deterministic_event_id;
use crate::{AttributeValue, OperationalEventV2, SignalKind};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use service_observability::SERVICE_LOG_SCHEMA_V1;

pub(crate) struct GenericLogDecoder<'a> {
    pub project: &'a str,
    pub environment: &'a str,
    pub format: LogFormat,
}

impl GenericLogDecoder<'_> {
    fn map(&self, record: &RawRecord) -> Result<OperationalEventV2> {
        let raw = std::str::from_utf8(&record.bytes).context("log text must be UTF-8")?;
        let parsed = serde_json::from_str::<Value>(raw);
        if matches!(self.format, LogFormat::Structured)
            || parsed
                .as_ref()
                .ok()
                .and_then(|v| v.get("schema"))
                .and_then(Value::as_str)
                == Some(SERVICE_LOG_SCHEMA_V1)
        {
            let mut event = decode_service_log_enriched(
                &record.bytes,
                &record.source_id,
                record.offset,
                self.project,
                self.environment,
                &record.enrichment,
            )?;
            event
                .attributes
                .insert("collector.raw".into(), AttributeValue::String(raw.into()));
            return Ok(event);
        }
        let document = match self.format {
            LogFormat::Json => parsed.context("invalid JSON log")?,
            LogFormat::Text => Value::Null,
            LogFormat::Auto => parsed.unwrap_or(Value::Null),
            LogFormat::Structured => unreachable!(),
        };
        let message = document
            .get("message")
            .or_else(|| document.get("msg"))
            .and_then(Value::as_str)
            .unwrap_or(raw.trim_end_matches(['\r', '\n']));
        let observed = record
            .enrichment
            .attributes
            .get("collector.observed_at")
            .and_then(AttributeValue::as_str)
            .map(DateTime::parse_from_rfc3339)
            .transpose()?
            .map(|v| v.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
        let mut event = OperationalEventV2::for_project_at(
            self.project,
            self.environment,
            deterministic_event_id(&record.source_id, record.offset, &record.bytes),
            SignalKind::Log,
            json!({"message": message, "raw": raw, "json": document}),
            observed,
        );
        if let Some(time) = document
            .get("timestamp")
            .or_else(|| document.get("time"))
            .and_then(Value::as_str)
        {
            event.occurred_at = DateTime::parse_from_rfc3339(time)
                .context("JSON log timestamp must be RFC3339")?
                .to_rfc3339();
        } else if let Some(AttributeValue::String(time)) =
            record.enrichment.attributes.get("collector.cri.timestamp")
        {
            event.occurred_at.clone_from(time);
        }
        event.severity = document
            .get("severity")
            .or_else(|| document.get("level"))
            .and_then(Value::as_str)
            .map(|v| v.to_ascii_uppercase());
        event.resource = record.enrichment.resource.clone();
        let service = document
            .get("service")
            .and_then(|v| v.as_str().or_else(|| v.get("name").and_then(Value::as_str)))
            .unwrap_or("unknown");
        event
            .resource
            .entry("service.name".into())
            .or_insert_with(|| service.into());
        event
            .resource
            .insert("collector.source_id".into(), record.source_id.clone());
        event.attributes = record.enrichment.attributes.clone();
        event.attributes.insert(
            "collector.source_id".into(),
            AttributeValue::String(record.source_id.clone()),
        );
        event.attributes.insert(
            "collector.offset".into(),
            AttributeValue::String(record.offset.to_string()),
        );
        for (key, value) in document.as_object().into_iter().flatten() {
            if let Some(value) = value.as_str() {
                event
                    .attributes
                    .insert(key.clone(), AttributeValue::String(value.into()));
            }
        }
        event.validate()?;
        Ok(event)
    }
}

impl RecordDecoder for GenericLogDecoder<'_> {
    type Error = QuarantineEntry;
    fn decode(&self, record: RawRecord) -> Result<OperationalEventV2, Self::Error> {
        self.map(&record).map_err(|error| {
            QuarantineEntry::invalid_line(
                &record.source_id,
                record.line,
                record.offset,
                "invalid_log",
                error.to_string(),
                &record.bytes,
            )
        })
    }
}
