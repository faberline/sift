//! Why ingest refused a request: its status, code, message and retry hint.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionStatus {
    BadRequest,
    PayloadTooLarge,
    UnsupportedMediaType,
    TooManyRequests,
    ServiceUnavailable,
}

impl AdmissionStatus {
    pub fn code(self) -> u16 {
        match self {
            Self::BadRequest => 400,
            Self::PayloadTooLarge => 413,
            Self::UnsupportedMediaType => 415,
            Self::TooManyRequests => 429,
            Self::ServiceUnavailable => 503,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AdmissionError {
    pub status: AdmissionStatus,
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    pub retry_after_secs: Option<u64>,
}

impl AdmissionError {
    pub(in crate::ingest) fn new(
        status: AdmissionStatus,
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
        Self::new(AdmissionStatus::BadRequest, code, message, false, None)
    }

    pub fn local_storage_backpressure(message: impl Into<String>) -> Self {
        Self::new(
            AdmissionStatus::ServiceUnavailable,
            "local_storage_backpressure",
            message,
            true,
            Some(5),
        )
    }
}
