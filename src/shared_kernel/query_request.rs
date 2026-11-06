//! The versioned query request: its AST version and limits, the time range,
//! mode, signal and metric function, and its validation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::shared_kernel::query_expression::QueryExpressionV1;
use crate::shared_kernel::query_limit::default_query_limit;

pub const QUERY_AST_VERSION: u32 = 1;
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
