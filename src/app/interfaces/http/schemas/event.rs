//! OpenAPI descriptions for event data. Runtime bytes use the original models.

use serde_json::Value;
use std::collections::BTreeMap;

fn is_false(value: &bool) -> bool {
    !*value
}

/// Phase-one signal kinds. Other SRE data is added only through a later
/// versioned public contract.
#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    Log,
    Span,
    Metric,
}

/// JSON representation of OpenTelemetry AnyValue semantics. Bytes are a
/// base64 string so JSON/protobuf transports preserve the distinction from a
/// normal string and from an array of integers.
#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum AttributeValue {
    String(String),
    Bool(bool),
    Int(i64),
    Double(f64),
    Bytes(String),
    Array(Vec<AttributeValue>),
    Map(BTreeMap<String, AttributeValue>),
}

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct InstrumentationScope {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, AttributeValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_url: Option<String>,
}

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetricTemporality {
    Delta,
    Cumulative,
    Gauge,
}

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct MetricExemplar {
    pub value: f64,
    pub trace_id: String,
    pub span_id: String,
}

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct MetricPoint {
    pub name: String,
    pub value: f64,
    /// Prometheus Remote Write uses a reserved NaN bit pattern to mark a
    /// series stale. Sift stores that state explicitly so durable JSON never
    /// contains a non-finite number.
    #[serde(default, skip_serializing_if = "is_false")]
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub temporality: MetricTemporality,
    #[serde(default)]
    pub exemplars: Vec<MetricExemplar>,
}

/// Durable content-addressed reference for a payload externalized before the
/// canonical raw event is acknowledged.
#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct ContentBlobRef {
    pub hash: String,
    pub size: u64,
    pub encoding: String,
}

/// The only event shape serialized by new journal writers.
#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct OperationalEventV2 {
    pub schema_version: u16,
    pub schema_url: String,
    pub event_id: String,
    pub project: String,
    pub environment: String,
    pub occurred_at: String,
    pub observed_at: String,
    pub signal: SignalKind,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub resource: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instrumentation_scope: Option<InstrumentationScope>,
    #[serde(default)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<MetricPoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blob_refs: Vec<ContentBlobRef>,
    #[schema(value_type = Object)]
    pub payload: Value,
}

schema_adapter!(crate::shared_kernel::event::SignalKind, SignalKind);
schema_adapter!(crate::shared_kernel::event::AttributeValue, AttributeValue);
schema_adapter!(
    crate::shared_kernel::event::InstrumentationScope,
    InstrumentationScope
);
schema_adapter!(
    crate::shared_kernel::event::MetricTemporality,
    MetricTemporality
);
schema_adapter!(crate::shared_kernel::event::MetricExemplar, MetricExemplar);
schema_adapter!(crate::shared_kernel::event::MetricPoint, MetricPoint);
schema_adapter!(crate::shared_kernel::event::ContentBlobRef, ContentBlobRef);
schema_adapter!(
    crate::shared_kernel::event::OperationalEventV2,
    OperationalEventV2
);
