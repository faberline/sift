//! The versioned query request: its AST version and limits, the time range,
//! mode, signal and metric function, and its validation.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::query::domain::filter_expression::{
    validate_field, QueryExpressionV1, MAX_FILTER_ARGUMENTS,
};

pub const QUERY_AST_VERSION: u32 = 1;
pub const DEFAULT_QUERY_LIMIT: usize = 100;
pub const MAX_QUERY_LIMIT: usize = 1_000;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimeRangeV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryModeV1 {
    #[default]
    Auto,
    Sync,
    Async,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricFunctionV1 {
    #[default]
    Raw,
    Sum,
    Avg,
    Min,
    Max,
    Count,
    Rate,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuerySignalV1 {
    Logs {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<QueryExpressionV1>,
    },
    Metrics {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default)]
        function: MetricFunctionV1,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_seconds: Option<u64>,
        #[serde(default)]
        group_by: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<QueryExpressionV1>,
    },
    Traces {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        service: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_duration_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_duration_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(default)]
        attributes: BTreeMap<String, serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<QueryExpressionV1>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueryRequestV1 {
    pub version: u32,
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default)]
    pub time_range: TimeRangeV1,
    pub signal: QuerySignalV1,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default)]
    pub mode: QueryModeV1,
}

impl QueryRequestV1 {
    pub fn validate(&self) -> Result<()> {
        if self.version != QUERY_AST_VERSION {
            bail!(
                "unsupported query version {}; expected {}",
                self.version,
                QUERY_AST_VERSION
            );
        }
        if self.project.trim().is_empty() {
            bail!("project must not be empty");
        }
        if self
            .environment
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            bail!("environment must not be empty");
        }
        if self.limit == 0 || self.limit > MAX_QUERY_LIMIT {
            bail!("limit must be between 1 and {MAX_QUERY_LIMIT}");
        }
        if let (Some(start), Some(end)) = (&self.time_range.start, &self.time_range.end) {
            let start = chrono::DateTime::parse_from_rfc3339(start)
                .context("time_range.start must be RFC 3339")?;
            let end = chrono::DateTime::parse_from_rfc3339(end)
                .context("time_range.end must be RFC 3339")?;
            if start >= end {
                bail!("time_range.start must be earlier than time_range.end");
            }
        } else {
            for (name, value) in [
                ("time_range.start", self.time_range.start.as_ref()),
                ("time_range.end", self.time_range.end.as_ref()),
            ] {
                if let Some(value) = value {
                    chrono::DateTime::parse_from_rfc3339(value)
                        .with_context(|| format!("{name} must be RFC 3339"))?;
                }
            }
        }
        match &self.signal {
            QuerySignalV1::Logs { filter }
            | QuerySignalV1::Metrics { filter, .. }
            | QuerySignalV1::Traces { filter, .. } => {
                if let Some(filter) = filter {
                    filter.validate(0)?;
                }
            }
        }
        if let QuerySignalV1::Metrics {
            step_seconds,
            group_by,
            ..
        } = &self.signal
        {
            if step_seconds.is_some_and(|step| step == 0) {
                bail!("step_seconds must be greater than zero");
            }
            if group_by.len() > MAX_FILTER_ARGUMENTS {
                bail!("group_by has too many fields");
            }
            for field in group_by {
                validate_field(field)?;
            }
        }
        if let QuerySignalV1::Traces {
            min_duration_ms,
            max_duration_ms,
            attributes,
            ..
        } = &self.signal
        {
            if min_duration_ms
                .zip(*max_duration_ms)
                .is_some_and(|(min, max)| min > max)
            {
                bail!("min_duration_ms must not exceed max_duration_ms");
            }
            if attributes.len() > MAX_FILTER_ARGUMENTS {
                bail!("attributes has too many fields");
            }
        }
        Ok(())
    }
}

fn default_query_limit() -> usize {
    DEFAULT_QUERY_LIMIT
}
