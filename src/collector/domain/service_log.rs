//! The rules a structured service-log line (StructuredServiceLogV1) must pass,
//! the stable id a stdout line gets from its source and offset, and
//! line-ending trimming.

use anyhow::{bail, Context, Result};
use chrono::DateTime;
use serde_json::Value;
use service_observability::{
    StructuredServiceLogV1, MAX_ATTRIBUTES, MAX_ATTRIBUTE_KEY_BYTES, MAX_ATTRIBUTE_VALUE_BYTES,
    MAX_EVENT_BYTES, MAX_REQUEST_ID_BYTES, SERVICE_LOG_SCHEMA_V1,
};
use sha2::{Digest, Sha256};

pub(in crate::collector) fn validate(log: &StructuredServiceLogV1) -> Result<()> {
    if log.schema != SERVICE_LOG_SCHEMA_V1 {
        bail!(
            "unsupported service log schema {}; expected {}",
            log.schema,
            SERVICE_LOG_SCHEMA_V1
        );
    }
    DateTime::parse_from_rfc3339(&log.timestamp)
        .context("service log timestamp must be RFC3339")?;
    if !matches!(
        log.severity.as_str(),
        "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR"
    ) {
        bail!("unsupported service log severity {}", log.severity);
    }
    bounded_nonempty("service.name", &log.service.name, MAX_EVENT_BYTES)?;
    bounded_nonempty("service.version", &log.service.version, MAX_EVENT_BYTES)?;
    bounded_nonempty("event", &log.event, MAX_EVENT_BYTES)?;
    if log.message.len() > MAX_ATTRIBUTE_VALUE_BYTES {
        bail!("service log message exceeds {MAX_ATTRIBUTE_VALUE_BYTES} bytes");
    }
    if log.attributes.len() > MAX_ATTRIBUTES {
        bail!("service log attributes exceed {MAX_ATTRIBUTES} entries");
    }
    for (key, value) in &log.attributes {
        bounded_nonempty("attribute key", key, MAX_ATTRIBUTE_KEY_BYTES)?;
        validate_attribute(value)
            .with_context(|| format!("invalid service log attribute {key}"))?;
    }
    validate_optional_hex("trace_id", log.trace_id.as_deref(), 32, true)?;
    validate_optional_hex("span_id", log.span_id.as_deref(), 16, true)?;
    validate_optional_hex("parent_span_id", log.parent_span_id.as_deref(), 16, true)?;
    validate_optional_hex("trace_flags", log.trace_flags.as_deref(), 2, false)?;
    if let Some(request_id) = log.request_id.as_deref() {
        if request_id.is_empty()
            || request_id.len() > MAX_REQUEST_ID_BYTES
            || request_id.chars().any(char::is_control)
        {
            bail!("invalid service log request_id");
        }
    }
    Ok(())
}

fn bounded_nonempty(name: &str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{name} must not be empty");
    }
    if value.len() > max {
        bail!("{name} exceeds {max} bytes");
    }
    Ok(())
}

fn validate_attribute(value: &Value) -> Result<()> {
    match value {
        Value::String(value) if value.len() > MAX_ATTRIBUTE_VALUE_BYTES => {
            bail!("string exceeds {MAX_ATTRIBUTE_VALUE_BYTES} bytes")
        }
        Value::String(_) | Value::Bool(_) | Value::Null => Ok(()),
        Value::Number(value) if value.as_f64().is_some_and(f64::is_finite) => Ok(()),
        Value::Number(_) => bail!("number must be finite"),
        Value::Array(_) | Value::Object(_) => {
            bail!("collector attributes must be primitive JSON values")
        }
    }
}

fn validate_optional_hex(
    name: &str,
    value: Option<&str>,
    len: usize,
    reject_zero: bool,
) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let valid = value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && (!reject_zero || value.bytes().any(|byte| byte != b'0'));
    if !valid {
        bail!("invalid service log {name}");
    }
    Ok(())
}

pub(in crate::collector) fn deterministic_event_id(
    source_id: &str,
    offset: u64,
    raw_line: &[u8],
) -> String {
    let mut digest = Sha256::new();
    digest.update(source_id.as_bytes());
    digest.update([0]);
    digest.update(offset.to_le_bytes());
    digest.update([0]);
    digest.update(raw_line);
    format!("stdout-{}", hex::encode(digest.finalize()))
}

pub(in crate::collector) fn trim_line_ending(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n")
        .unwrap_or(line)
        .strip_suffix(b"\r")
        .unwrap_or_else(|| line.strip_suffix(b"\n").unwrap_or(line))
}
