//! A log record as the logging projection keeps and returns it, the query and
//! page over records, the filter a record must pass, and the logging state.

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Utc};
use thiserror::Error;

pub const LOGGING_SCHEMA_VERSION: u32 = 3;
pub const DEFAULT_RETAINED_LOG_RECORDS: usize = 100_000;

pub(in crate::projection) const RESOURCE_TYPE: &str = "gcp.resource.type";
pub(in crate::projection) const SERVICE_NAME: &str = "service.name";

impl LogQuery {
    pub(in crate::projection) fn validate(&self) -> Result<QueryBounds, LogQueryError> {
        if self.project.trim().is_empty() {
            return Err(LogQueryError::EmptyProject);
        }
        if self.limit == 0 || self.limit > MAX_LOG_QUERY_LIMIT {
            return Err(LogQueryError::Limit);
        }
        let start = parse_optional_time("start_time", self.start_time.as_deref())?;
        let end = parse_optional_time("end_time", self.end_time.as_deref())?;
        if start.zip(end).is_some_and(|(start, end)| start >= end) {
            return Err(LogQueryError::TimeOrder);
        }
        Ok(QueryBounds { start, end })
    }
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

#[derive(Debug, Error)]
pub(in crate::projection) enum LogQueryError {
    #[error("project must not be empty")]
    EmptyProject,
    #[error("limit must be between 1 and {max}", max = MAX_LOG_QUERY_LIMIT)]
    Limit,
    #[error("{name} must be RFC3339")]
    TimeNotRfc3339 {
        name: &'static str,
        #[source]
        source: chrono::ParseError,
    },
    #[error("start_time must be earlier than end_time")]
    TimeOrder,
}

fn parse_optional_time(
    name: &'static str,
    value: Option<&str>,
) -> Result<Option<DateTime<Utc>>, LogQueryError> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map_err(|source| LogQueryError::TimeNotRfc3339 { name, source })
                .map(|value| value.with_timezone(&Utc))
        })
        .transpose()
}

pub(in crate::projection) fn projection_row_key(cursor: u64) -> String {
    format!("cursor-{cursor:020}")
}

pub use crate::shared_kernel::projection_log::{
    LogPage, LogQuery, LogRecordV1, MAX_LOG_QUERY_LIMIT, PROJECTION_LOGGING_STORE,
};
