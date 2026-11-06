//! Render pure query failures with the existing HTTP envelope.

use crate::app::interfaces::http::api_error::ApiError;
use crate::query::domain::query_failure::QueryFailure;

impl From<QueryFailure> for ApiError {
    fn from(error: QueryFailure) -> Self {
        Self::bad_request(error.code, error.message)
    }
}
