//! Why an operational event failed validation or decode. The text of each
//! error is the HTTP 400 body a rejected event returns.

use thiserror::Error;

use crate::shared_kernel::event::EVENT_SCHEMA_VERSION;

#[derive(Debug, Error)]
pub enum EventError {
    #[error("attribute double values must be finite")]
    NonFiniteAttributeDouble,
    #[error("attribute map keys must not be empty")]
    EmptyAttributeMapKey,
    #[error("unsupported schema_version {0}; expected {expected}", expected = EVENT_SCHEMA_VERSION)]
    UnsupportedSchemaVersion(u16),
    #[error("{0} must not be empty")]
    EmptyField(&'static str),
    #[error("occurred_at must be RFC3339")]
    OccurredAtNotRfc3339(#[source] chrono::ParseError),
    #[error("observed_at must be RFC3339")]
    ObservedAtNotRfc3339(#[source] chrono::ParseError),
    #[error("resource must contain at least one stable identity field")]
    EmptyResource,
    #[error("payload must not be null")]
    NullPayload,
    #[error("instrumentation_scope.name must not be empty")]
    EmptyScopeName,
    #[error("attribute keys must not be empty")]
    EmptyAttributeKey,
    #[error("metric signals require a direct metric point")]
    MissingMetricPoint,
    #[error("only metric signals may contain a metric point")]
    UnexpectedMetricPoint,
    #[error("blob hash must use the sha256:<hex> form")]
    BlobHashForm,
    #[error("blob hash must contain a 64-character SHA-256 digest")]
    BlobHashDigest,
    #[error("blob size must be greater than zero")]
    EmptyBlob,
    #[error("blob encoding must not be empty")]
    EmptyBlobEncoding,
    #[error("blob references must not contain duplicate hashes")]
    DuplicateBlobHash,
    #[error("metric name must be non-empty and value must be finite")]
    InvalidMetricPoint,
    #[error("stale metric points must use a zero storage value")]
    NonZeroStaleMetric,
    #[error("metric exemplars require finite value, trace_id, and span_id")]
    InvalidMetricExemplar,
    #[error("decode operational event: {0}")]
    Decode(serde_json::Error),
    #[error("operational event requires integer schema_version")]
    MissingSchemaVersion,
    #[error("schema_version exceeds u16")]
    SchemaVersionTooLarge(#[source] std::num::TryFromIntError),
    #[error("unsupported schema_version {0}")]
    UnsupportedDecodedSchemaVersion(u16),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
