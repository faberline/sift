//! An async query job: its status, its record, and the ids it may have.

use thiserror::Error;

pub use crate::shared_kernel::query_job::{QueryJobStatusV1, QueryJobV1};

#[derive(Debug, Error)]
#[error("query id has an invalid format")]
pub(in crate::query) struct InvalidQueryId;

pub(in crate::query) fn validate_id(query_id: &str) -> Result<(), InvalidQueryId> {
    if query_id.len() != 32 || !query_id.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err(InvalidQueryId);
    }
    Ok(())
}
