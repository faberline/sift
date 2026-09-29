//! The collector: it reads service logs from a file, stdin or a node's CRI
//! log root, validates and maps each line to an operational event,
//! quarantines what it rejects, delivers the rest to Sift over OTLP, and
//! checkpoints only what Sift has acknowledged.

pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;

pub use application::run_collector::{run_collector, CollectorSummary};
pub use domain::config::{
    CollectorConfig, CriMetadata, CriSourceConfig, SourceSpec, DEFAULT_BATCH_SIZE,
    DEFAULT_MAX_LINE_BYTES, DEFAULT_MAX_RETRIES, MAX_BATCH_SIZE, MAX_LINE_BYTES,
};
pub use domain::quarantine::QuarantineEntry;
pub use infrastructure::checkpoint::CollectorCheckpoint;
pub use infrastructure::service_log_mapper::decode_service_log;
