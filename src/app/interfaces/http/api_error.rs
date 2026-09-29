//! The error every HTTP handler returns, and how it becomes service_http's
//! detailed error envelope with its retry and projection-lag metadata.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use service_http::ProjectionMetadata;
use service_projection::ProjectionLag;

use crate::ingest::domain::admission_error::AdmissionError;

pub(crate) struct ApiError {
    status: StatusCode,
    error: &'static str,
    pub(crate) message: String,
    retryable: bool,
    retry_after_secs: Option<u64>,
    projection_lag: Option<Box<ProjectionLag>>,
}

impl ApiError {
    pub(crate) fn bad_request(error: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error,
            message: message.into(),
            retryable: false,
            retry_after_secs: None,
            projection_lag: None,
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: "journal_failure",
            message: message.into(),
            retryable: true,
            retry_after_secs: Some(1),
            projection_lag: None,
        }
    }

    pub(crate) fn temporarily_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error: "retention_checkpoint_pending",
            message: message.into(),
            retryable: true,
            retry_after_secs: Some(1),
            projection_lag: None,
        }
    }

    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            error: "project_forbidden",
            message: message.into(),
            retryable: false,
            retry_after_secs: None,
            projection_lag: None,
        }
    }

    pub(crate) fn not_found(error: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            error,
            message: message.into(),
            retryable: false,
            retry_after_secs: None,
            projection_lag: None,
        }
    }

    pub(crate) fn unsupported_media(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            error: "unsupported_media_type",
            message: message.into(),
            retryable: false,
            retry_after_secs: None,
            projection_lag: None,
        }
    }

    pub(crate) fn from_admission(error: AdmissionError) -> Self {
        Self {
            status: error.status,
            error: error.code,
            message: error.message,
            retryable: error.retryable,
            retry_after_secs: error.retry_after_secs,
            projection_lag: None,
        }
    }

    pub(crate) fn projection_lag(lag: ProjectionLag) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error: "projection_lag",
            message: format!(
                "projection `{}` is at cursor {}, below required cursor {}",
                lag.projection, lag.current_cursor, lag.required_cursor
            ),
            retryable: true,
            retry_after_secs: Some(lag.retry_after_seconds),
            projection_lag: Some(Box::new(lag)),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let lag = self.projection_lag;
        let message = if self.retryable {
            format!("{} (retryable)", self.message)
        } else {
            self.message
        };
        let mut error = service_http::ApiErr::new(self.status, self.error, message)
            .with_retryable(self.retryable);
        if let Some(seconds) = self.retry_after_secs {
            error = error.with_retry_after_seconds(seconds);
        }
        if let Some(lag) = lag {
            error = error.with_projection(ProjectionMetadata {
                projection: lag.projection,
                required_cursor: lag.required_cursor,
                current_cursor: lag.current_cursor,
            });
        }
        error.into_response()
    }
}
