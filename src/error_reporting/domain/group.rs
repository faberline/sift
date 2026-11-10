//! Stable grouping excludes volatile identifiers from messages and stacks.
use crate::shared_kernel::{projection_log::LogRecordV1, sha256_hex::sha256_hex};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, sync::OnceLock};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorStatus {
    Open,
    Resolved,
    Ignored,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ErrorGroup {
    pub id: String,
    pub project: String,
    pub environment: String,
    pub service: String,
    pub exception_type: String,
    pub fingerprint: String,
    pub status: ErrorStatus,
    pub count: u64,
    pub first_seen: String,
    pub last_seen: String,
    pub first_cursor: u64,
    pub last_cursor: u64,
    pub recurrence_count: u64,
    pub recurrence_cursor: Option<u64>,
    pub versions: BTreeSet<String>,
    pub samples: Vec<Value>,
}

fn field(record: &LogRecordV1, name: &str) -> Option<String> {
    record
        .attributes
        .get(name)
        .and_then(|v| v.as_str().map(str::to_owned))
        .or_else(|| {
            record
                .json_payload
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            record
                .json_payload
                .get("json")
                .and_then(|v| v.get(name))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .or_else(|| {
            let (object, key) = name.split_once('.')?;
            [
                &record.json_payload,
                record.json_payload.get("json").unwrap_or(&Value::Null),
            ]
            .into_iter()
            .find_map(|document| document.get(object)?.get(key)?.as_str().map(str::to_owned))
        })
}

pub(crate) fn normalized(text: &str) -> String {
    static VOLATILE: OnceLock<Regex> = OnceLock::new();
    let pattern = VOLATILE.get_or_init(|| Regex::new(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b|\b0x[0-9a-f]+\b|\b\d+(?:\.\d+)?\b").expect("error normalization regex"));
    pattern
        .replace_all(text, "?")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl ErrorGroup {
    pub(crate) fn from_log(record: &LogRecordV1) -> Option<Self> {
        let exception_type = field(record, "exception.type").unwrap_or_else(|| "log_error".into());
        if exception_type == "log_error"
            && !record.severity.as_deref().is_some_and(|s| {
                matches!(
                    s.to_ascii_uppercase().as_str(),
                    "ERROR" | "FATAL" | "CRITICAL"
                )
            })
        {
            return None;
        }
        let service = record
            .resource
            .get("service.name")
            .cloned()
            .unwrap_or_else(|| "unknown".into());
        let text = field(record, "exception.stacktrace")
            .or_else(|| field(record, "exception.message"))
            .unwrap_or_else(|| record.body_text.clone());
        let fingerprint = normalized(&text);
        let id = sha256_hex(
            serde_json::to_string(&(
                &record.project,
                &record.environment,
                &service,
                &exception_type,
                &fingerprint,
            ))
            .expect("error fingerprint")
            .as_bytes(),
        );
        let mut group = Self {
            id,
            project: record.project.clone(),
            environment: record.environment.clone(),
            service,
            exception_type,
            fingerprint,
            status: ErrorStatus::Open,
            count: 0,
            first_seen: record.occurred_at.clone(),
            last_seen: record.occurred_at.clone(),
            first_cursor: record.cursor,
            last_cursor: 0,
            recurrence_count: 0,
            recurrence_cursor: None,
            versions: BTreeSet::new(),
            samples: Vec::new(),
        };
        group.observe(record).ok()?;
        Some(group)
    }

    pub(crate) fn observe(&mut self, record: &LogRecordV1) -> Result<(), String> {
        if self.count > 0 && record.cursor <= self.last_cursor {
            return Ok(());
        }
        self.count = self
            .count
            .checked_add(1)
            .ok_or("error group count overflow")?;
        self.last_cursor = record.cursor;
        let occurred =
            chrono::DateTime::parse_from_rfc3339(&record.occurred_at).map_err(|e| e.to_string())?;
        let first =
            chrono::DateTime::parse_from_rfc3339(&self.first_seen).map_err(|e| e.to_string())?;
        let last =
            chrono::DateTime::parse_from_rfc3339(&self.last_seen).map_err(|e| e.to_string())?;
        if occurred < first {
            self.first_seen = record.occurred_at.clone();
        }
        if occurred > last {
            self.last_seen = record.occurred_at.clone();
        }
        if self.status == ErrorStatus::Resolved {
            self.status = ErrorStatus::Open;
            self.recurrence_count = self
                .recurrence_count
                .checked_add(1)
                .ok_or("error recurrence overflow")?;
            self.recurrence_cursor = Some(record.cursor);
        }
        if let Some(version) = record
            .resource
            .get("service.version")
            .cloned()
            .or_else(|| field(record, "version"))
        {
            if self.versions.len() < 100 {
                self.versions.insert(version);
            }
        }
        let original = serde_json::to_value(record).map_err(|e| e.to_string())?;
        let sample = if serde_json::to_vec(&original)
            .map_err(|e| e.to_string())?
            .len()
            <= 64 * 1024
        {
            original
        } else {
            serde_json::json!({"event_id":record.event_id,"cursor":record.cursor,"sample_omitted":true,"reason":"sample exceeds 64 KiB; read original log"})
        };
        self.samples.push(sample);
        if self.samples.len() > 3 {
            self.samples.remove(0);
        }
        Ok(())
    }
}
