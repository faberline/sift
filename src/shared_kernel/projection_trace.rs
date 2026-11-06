//! Published trace read data. Field order and serialization are stable.

use crate::shared_kernel::event::{AttributeValue, InstrumentationScope};
use crate::shared_kernel::query_limit::default_query_limit;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROJECTION_TRACE_STORE: &str = "trace-store";
pub const MAX_TRACE_QUERY_LIMIT: usize = 1_000;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct SpanLinkV1 {
    pub trace_id: String,
    pub span_id: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, AttributeValue>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct SpanEventV1 {
    pub name: String,
    pub time_unix_nano: u64,
    #[serde(default)]
    pub attributes: BTreeMap<String, AttributeValue>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
    pub attributes: BTreeMap<String, AttributeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrumentation_scope: Option<InstrumentationScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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
    pub attributes: BTreeMap<String, AttributeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_trace_id: Option<String>,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TracePage {
    pub traces: Vec<TraceResultV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_trace_id: Option<String>,
    pub projection_cursor: u64,
    pub has_more: bool,
}

impl TraceQuery {
    pub fn for_project(project: impl Into<String>) -> Self {
        Self {
            project: project.into(),
            environment: None,
            start_time_unix_nano: None,
            end_time_unix_nano: None,
            service: None,
            operation: None,
            min_duration_unix_nano: None,
            max_duration_unix_nano: None,
            status: None,
            attributes: BTreeMap::new(),
            after_trace_id: None,
            limit: default_query_limit(),
        }
    }
}
