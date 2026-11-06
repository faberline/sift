//! The phase-one wire schemas beside the versioned query: the log tail
//! request, the correlation request and response, and the service listing
//! query and response.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::shared_kernel::projection_log::LogRecordV1;
use crate::shared_kernel::projection_metric::MetricSeriesResultV1;
use crate::shared_kernel::projection_trace::TraceResultV1;
use crate::shared_kernel::query_expression::QueryExpressionV1;
use crate::shared_kernel::query_limit::default_query_limit;
use crate::shared_kernel::query_request::TimeRangeV1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogTailRequestV1 {
    pub version: u32,
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<QueryExpressionV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_cursor: Option<String>,
    #[serde(default = "default_tail_wait_ms")]
    pub wait_ms: u64,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorrelationRequestV1 {
    pub version: u32,
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default)]
    pub time_range: TimeRangeV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, serde_json::Value>,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct CorrelationResponseV1 {
    pub logs: Vec<LogRecordV1>,
    pub metrics: Vec<MetricSeriesResultV1>,
    pub traces: Vec<TraceResultV1>,
    pub watermark: u64,
    pub partial: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceQueryV1 {
    pub project: String,
    #[serde(default)]
    pub environment: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ServiceSummaryV1 {
    pub name: String,
    pub environments: Vec<String>,
    pub versions: Vec<String>,
    pub signals: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ServiceListResponseV1 {
    pub services: Vec<ServiceSummaryV1>,
    pub watermark: u64,
}

fn default_tail_wait_ms() -> u64 {
    2_000
}
