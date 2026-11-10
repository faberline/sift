//! The V2 request adds bounded metric expressions and exact log counts.
use super::query_expression::QueryExpressionV1;
use super::query_request::TimeRangeV1;
use super::query_response::QueryResponseV1;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueryRequestV2 {
    pub version: u32,
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default)]
    pub time_range: TimeRangeV1,
    pub signal: QuerySignalV2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_seconds: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuerySignalV2 {
    Metrics {
        expression: String,
    },
    LogsCount {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<Box<QueryExpressionV1>>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct QueryResponseV2 {
    pub version: u32,
    #[serde(flatten)]
    pub response: QueryResponseV1,
}
