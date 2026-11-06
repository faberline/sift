//! OpenAPI descriptions for trace data. Runtime bytes use the original models.

use crate::{AttributeValue, InstrumentationScope};
use std::collections::BTreeMap;

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct SpanLinkV1 {
    pub trace_id: String,
    pub span_id: String,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, AttributeValue>,
}

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct SpanEventV1 {
    pub name: String,
    pub time_unix_nano: u64,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, AttributeValue>,
}

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct SpanRecordV1 {
    pub cursor: u64,
    pub event_id: String,
    pub project: String,
    pub environment: String,
    pub trace_id: String,
    pub span_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    #[serde(default)]
    pub links: Vec<SpanLinkV1>,
    #[serde(default)]
    pub events: Vec<SpanEventV1>,
    pub resource: BTreeMap<String, String>,
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, AttributeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrumentation_scope: Option<InstrumentationScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct TraceResultV1 {
    pub project: String,
    pub trace_id: String,
    pub spans: Vec<SpanRecordV1>,
    pub root_span_ids: Vec<String>,
    pub partial: bool,
    pub gaps: Vec<String>,
    pub cycles: Vec<Vec<String>>,
    pub critical_path_span_ids: Vec<String>,
    pub duration_unix_nano: u64,
    pub correlation_ids: BTreeMap<String, Vec<String>>,
    pub projection_cursor: u64,
}

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct TraceQuery {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time_unix_nano: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time_unix_nano: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_duration_unix_nano: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_duration_unix_nano: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, AttributeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_trace_id: Option<String>,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
}

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct TracePage {
    pub traces: Vec<TraceResultV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_trace_id: Option<String>,
    pub projection_cursor: u64,
    pub has_more: bool,
}

schema_adapter!(crate::projection::domain::trace::SpanLinkV1, SpanLinkV1);
schema_adapter!(crate::projection::domain::trace::SpanEventV1, SpanEventV1);
schema_adapter!(crate::projection::domain::trace::SpanRecordV1, SpanRecordV1);
schema_adapter!(
    crate::projection::domain::trace::TraceResultV1,
    TraceResultV1
);
schema_adapter!(crate::projection::domain::trace::TraceQuery, TraceQuery);
schema_adapter!(crate::projection::domain::trace::TracePage, TracePage);
