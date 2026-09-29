//! Ingest: telemetry admitted within bounded limits, decoded from OTLP, Cloud
//! Logging structured JSON and Prometheus remote write into operational
//! events, governed, and appended through the group-commit queue to the
//! replicated journal, behind the OTLP/HTTP, OTLP/gRPC and remote-write
//! endpoints.
//!
//! `batch`, `gcp`, `limits` and `otlp` keep their public paths as re-exports.

pub(crate) mod application;
pub mod batch;
pub(crate) mod domain;
pub mod gcp;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;
pub mod limits;
pub mod otlp;

pub use crate::ingest::interfaces::http::batch::{
    BatchItemResult, BatchOutcome, EventWriteRequest, EventWriteResponse, IngestErrorDetail,
};
pub use crate::ingest::{
    application::admission_controller::{AdmissionController, AdmissionPermit},
    domain::{admission_error::AdmissionError, ingest_limits::IngestLimits},
};
