//! The ingest limits' public paths. The ingest context now owns the limits,
//! the admission error and the admission controller; these re-exports keep
//! `sift::ingest::limits::*` compiling for callers outside the crate.

pub use crate::ingest::application::admission_controller::{AdmissionController, AdmissionPermit};
pub use crate::ingest::domain::admission_error::AdmissionError;
pub use crate::ingest::domain::ingest_limits::IngestLimits;
