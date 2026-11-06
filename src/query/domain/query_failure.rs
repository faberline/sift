//! Query validation failures with stable public codes and text.

use thiserror::Error;

#[derive(Debug, Error)]
#[error("{message}")]
pub(crate) struct QueryFailure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl QueryFailure {
    pub(crate) fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
