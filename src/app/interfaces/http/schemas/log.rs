//! OpenAPI descriptions for log data. Runtime bytes use the original models.

use crate::AttributeValue;
use std::collections::BTreeMap;

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
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

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
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

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct LogPage {
    pub records: Vec<LogRecordV1>,
    pub next_cursor: u64,
    pub projection_cursor: u64,
    pub has_more: bool,
}

schema_adapter!(crate::projection::domain::log::LogRecordV1, LogRecordV1);
schema_adapter!(crate::projection::domain::log::LogQuery, LogQuery);
schema_adapter!(crate::projection::domain::log::LogPage, LogPage);
