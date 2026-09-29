//! A log record as the logging projection keeps and returns it, the query and
//! page over records, the filter a record must pass, and the logging state.

use std::collections::{BTreeMap, HashSet};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AttributeValue;

pub const PROJECTION_LOGGING_STORE: &str = "logging-store";
pub const LOGGING_SCHEMA_VERSION: u32 = 3;
pub const DEFAULT_RETAINED_LOG_RECORDS: usize = 100_000;
pub const MAX_LOG_QUERY_LIMIT: usize = 1_000;

pub(in crate::projection) const RESOURCE_TYPE: &str = "gcp.resource.type";
pub(in crate::projection) const SERVICE_NAME: &str = "service.name";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct LogRecordV1 {
    pub cursor: u64,
    pub event_id: String,
    pub project: String,
    pub environment: String,
    pub occurred_at: String,
    pub observed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    pub body_text: String,
    #[schema(value_type = Object)]
    pub json_payload: serde_json::Value,
    pub resource: BTreeMap<String, String>,
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, AttributeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub coexistence_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct LogQuery {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub attribute_equals: BTreeMap<String, AttributeValue>,
    #[serde(default)]
    pub after_cursor: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_cursor: Option<u64>,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
}

impl LogQuery {
    pub fn for_project(project: impl Into<String>) -> Self {
        Self {
            project: project.into(),
            environment: None,
            start_time: None,
            end_time: None,
            severity: None,
            resource_type: None,
            service_name: None,
            trace_id: None,
            span_id: None,
            request_id: None,
            session_id: None,
            text: None,
            attribute_equals: BTreeMap::new(),
            after_cursor: 0,
            min_cursor: None,
            limit: default_query_limit(),
        }
    }

    pub(in crate::projection) fn validate(&self) -> Result<QueryBounds> {
        if self.project.trim().is_empty() {
            bail!("project must not be empty");
        }
        if self.limit == 0 || self.limit > MAX_LOG_QUERY_LIMIT {
            bail!("limit must be between 1 and {MAX_LOG_QUERY_LIMIT}");
        }
        let start = parse_optional_time("start_time", self.start_time.as_deref())?;
        let end = parse_optional_time("end_time", self.end_time.as_deref())?;
        if start.zip(end).is_some_and(|(start, end)| start >= end) {
            bail!("start_time must be earlier than end_time");
        }
        Ok(QueryBounds { start, end })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct LogPage {
    pub records: Vec<LogRecordV1>,
    pub next_cursor: u64,
    pub projection_cursor: u64,
    pub has_more: bool,
}

#[derive(Default)]
pub(in crate::projection) struct LoggingState {
    pub(in crate::projection) records: BTreeMap<u64, LogRecordV1>,
    pub(in crate::projection) projection_cursor: u64,
}

pub(in crate::projection) struct QueryBounds {
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
}

pub(in crate::projection) fn record_matches(
    record: &LogRecordV1,
    query: &LogQuery,
    bounds: &QueryBounds,
    candidates: Option<&HashSet<String>>,
) -> bool {
    if record.project != query.project
        || query
            .environment
            .as_ref()
            .is_some_and(|value| record.environment != *value)
        || query.severity.as_ref().is_some_and(|value| {
            !record
                .severity
                .as_deref()
                .is_some_and(|severity| severity.eq_ignore_ascii_case(value))
        })
        || query
            .resource_type
            .as_ref()
            .is_some_and(|value| record.resource.get(RESOURCE_TYPE) != Some(value))
        || query
            .service_name
            .as_ref()
            .is_some_and(|value| record.resource.get(SERVICE_NAME) != Some(value))
        || !matches_optional(&record.trace_id, &query.trace_id)
        || !matches_optional(&record.span_id, &query.span_id)
        || !matches_optional(&record.request_id, &query.request_id)
        || !matches_optional(&record.session_id, &query.session_id)
        || !query
            .attribute_equals
            .iter()
            .all(|(key, value)| record.attributes.get(key) == Some(value))
        || candidates.is_some_and(|ids| !ids.contains(&projection_row_key(record.cursor)))
    {
        return false;
    }
    let Ok(occurred_at) = DateTime::parse_from_rfc3339(&record.occurred_at) else {
        return false;
    };
    let occurred_at = occurred_at.with_timezone(&Utc);
    bounds.start.is_none_or(|start| occurred_at >= start)
        && bounds.end.is_none_or(|end| occurred_at < end)
}

fn matches_optional(actual: &Option<String>, expected: &Option<String>) -> bool {
    expected
        .as_ref()
        .is_none_or(|value| actual.as_ref() == Some(value))
}

fn parse_optional_time(name: &str, value: Option<&str>) -> Result<Option<DateTime<Utc>>> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .with_context(|| format!("{name} must be RFC3339"))
                .map(|value| value.with_timezone(&Utc))
        })
        .transpose()
}

fn default_query_limit() -> usize {
    100
}

pub(in crate::projection) fn projection_row_key(cursor: u64) -> String {
    format!("cursor-{cursor:020}")
}
