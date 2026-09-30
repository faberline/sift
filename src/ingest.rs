//! Ingest: telemetry admitted within bounded limits, decoded from OTLP and
//! Prometheus remote write into operational events, governed, and appended
//! through the group-commit queue to the replicated journal, behind the
//! OTLP/HTTP, OTLP/gRPC and remote-write endpoints.
//!
//! `limits` and `otlp` keep their public paths as re-exports.

pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;
pub mod limits;
pub mod otlp;

pub use crate::ingest::{
    application::admission_controller::{AdmissionController, AdmissionPermit},
    domain::{admission_error::AdmissionError, ingest_limits::IngestLimits},
    interfaces::remote_write::remote_write_consumer::{decode_remote_write, DecodedWrite},
};
