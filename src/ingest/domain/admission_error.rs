//! Why ingest refused a request: its status, code, message and retry hint.

use axum::http::StatusCode;

#[derive(Clone, Debug)]
pub struct AdmissionError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    pub retry_after_secs: Option<u64>,
}

impl AdmissionError {
    pub(in crate::ingest) fn new(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
        retryable: bool,
        retry_after_secs: Option<u64>,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retryable,
            retry_after_secs,
        }
    }

    pub fn invalid(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message, false, None)
    }

    pub fn local_storage_backpressure(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "local_storage_backpressure",
            message,
            true,
            Some(5),
        )
    }
}
