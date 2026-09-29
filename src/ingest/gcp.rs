//! GCP structured-log normalization's public paths. The ingest context now
//! owns it; these re-exports keep `sift::ingest::gcp::*` compiling for callers
//! outside the crate.

pub use crate::ingest::infrastructure::gcp::{looks_like_structured_log, normalize_structured_log};
