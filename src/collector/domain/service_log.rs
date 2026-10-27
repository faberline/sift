//! The rules a structured service-log line (StructuredServiceLogV1) must pass,
//! the stable id a stdout line gets from its source and offset, and
//! line-ending trimming.

use chrono::DateTime;
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub(in crate::collector) struct ServiceLogView<'a> {
    pub schema: &'a str,
    pub timestamp: &'a str,
    pub severity: &'a str,
    pub service_name: &'a str,
    pub service_version: &'a str,
    pub event: &'a str,
    pub message: &'a str,
    pub attributes: &'a std::collections::BTreeMap<String, Value>,
    pub trace_id: Option<&'a str>,
    pub span_id: Option<&'a str>,
    pub parent_span_id: Option<&'a str>,
    pub trace_flags: Option<&'a str>,
    pub request_id: Option<&'a str>,
}

pub(in crate::collector) struct ServiceLogLimits {
    pub schema: &'static str,
    pub max_attributes: usize,
    pub max_attribute_key_bytes: usize,
    pub max_attribute_value_bytes: usize,
    pub max_event_bytes: usize,
    pub max_request_id_bytes: usize,
}

#[derive(Debug, Error)]
pub(in crate::collector) enum ServiceLogError {
    #[error("unsupported service log schema {actual}; expected {expected}")]
    UnsupportedSchema {
        actual: String,
        expected: &'static str,
    },
    #[error("service log timestamp must be RFC3339")]
    TimestampNotRfc3339(#[source] chrono::ParseError),
    #[error("unsupported service log severity {0}")]
    UnsupportedSeverity(String),
    #[error("service log message exceeds {max} bytes")]
    MessageTooLong { max: usize },
    #[error("service log attributes exceed {max} entries")]
    TooManyAttributes { max: usize },
    #[error("invalid service log attribute {key}")]
    InvalidAttribute {
        key: String,
        #[source]
        source: Box<ServiceLogError>,
    },
    #[error("invalid service log request_id")]
    InvalidRequestId,
    #[error("{0} must not be empty")]
    Empty(String),
    #[error("{name} exceeds {max} bytes")]
    TooLong { name: String, max: usize },
    #[error("string exceeds {max} bytes")]
    StringTooLong { max: usize },
    #[error("number must be finite")]
    NonFiniteNumber,
    #[error("collector attributes must be primitive JSON values")]
    NotPrimitive,
    #[error("invalid service log {0}")]
    InvalidHex(String),
}

pub(in crate::collector) fn validate(
    log: ServiceLogView<'_>,
    limits: ServiceLogLimits,
) -> Result<(), ServiceLogError> {
    if log.schema != limits.schema {
        return Err(ServiceLogError::UnsupportedSchema {
            actual: log.schema.to_owned(),
            expected: limits.schema,
        });
    }
    DateTime::parse_from_rfc3339(log.timestamp).map_err(ServiceLogError::TimestampNotRfc3339)?;
    if !matches!(log.severity, "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR") {
        return Err(ServiceLogError::UnsupportedSeverity(
            log.severity.to_owned(),
        ));
    }
    bounded_nonempty("service.name", log.service_name, limits.max_event_bytes)?;
    bounded_nonempty(
        "service.version",
        log.service_version,
        limits.max_event_bytes,
    )?;
    bounded_nonempty("event", log.event, limits.max_event_bytes)?;
    if log.message.len() > limits.max_attribute_value_bytes {
        return Err(ServiceLogError::MessageTooLong {
            max: limits.max_attribute_value_bytes,
        });
    }
    if log.attributes.len() > limits.max_attributes {
        return Err(ServiceLogError::TooManyAttributes {
            max: limits.max_attributes,
        });
    }
    for (key, value) in log.attributes {
        bounded_nonempty("attribute key", key, limits.max_attribute_key_bytes)?;
        validate_attribute(value, limits.max_attribute_value_bytes).map_err(|source| {
            ServiceLogError::InvalidAttribute {
                key: key.clone(),
                source: Box::new(source),
            }
        })?;
    }
    validate_optional_hex("trace_id", log.trace_id, 32, true)?;
    validate_optional_hex("span_id", log.span_id, 16, true)?;
    validate_optional_hex("parent_span_id", log.parent_span_id, 16, true)?;
    validate_optional_hex("trace_flags", log.trace_flags, 2, false)?;
    if let Some(request_id) = log.request_id {
        if request_id.is_empty()
            || request_id.len() > limits.max_request_id_bytes
            || request_id.chars().any(char::is_control)
        {
            return Err(ServiceLogError::InvalidRequestId);
        }
    }
    Ok(())
}

fn bounded_nonempty(name: &str, value: &str, max: usize) -> Result<(), ServiceLogError> {
    if value.trim().is_empty() {
        return Err(ServiceLogError::Empty(name.to_owned()));
    }
    if value.len() > max {
        return Err(ServiceLogError::TooLong {
            name: name.to_owned(),
            max,
        });
    }
    Ok(())
}

fn validate_attribute(value: &Value, max: usize) -> Result<(), ServiceLogError> {
    match value {
        Value::String(value) if value.len() > max => Err(ServiceLogError::StringTooLong { max }),
        Value::String(_) | Value::Bool(_) | Value::Null => Ok(()),
        Value::Number(value) if value.as_f64().is_some_and(f64::is_finite) => Ok(()),
        Value::Number(_) => Err(ServiceLogError::NonFiniteNumber),
        Value::Array(_) | Value::Object(_) => Err(ServiceLogError::NotPrimitive),
    }
}

fn validate_optional_hex(
    name: &str,
    value: Option<&str>,
    len: usize,
    reject_zero: bool,
) -> Result<(), ServiceLogError> {
    let Some(value) = value else {
        return Ok(());
    };
    let valid = value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && (!reject_zero || value.bytes().any(|byte| byte != b'0'));
    if !valid {
        return Err(ServiceLogError::InvalidHex(name.to_owned()));
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
