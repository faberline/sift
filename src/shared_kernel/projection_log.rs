//! Published log read data. Field order and serialization are stable.

use crate::shared_kernel::event::AttributeValue;
use crate::shared_kernel::query_limit::default_query_limit;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROJECTION_LOGGING_STORE: &str = "logging-store";
pub const MAX_LOG_QUERY_LIMIT: usize = 1_000;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
    pub json_payload: serde_json::Value,
    pub resource: BTreeMap<String, String>,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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
    pub attribute_equals: BTreeMap<String, AttributeValue>,
    #[serde(default)]
    pub after_cursor: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_cursor: Option<u64>,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LogPage {
    pub records: Vec<LogRecordV1>,
    pub next_cursor: u64,
    pub projection_cursor: u64,
    pub has_more: bool,
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
}
