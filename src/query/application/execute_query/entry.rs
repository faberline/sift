//! Count failed or partial V1 requests at the shared service boundary.
use crate::app::{interfaces::http::api_error::ApiError, service_state::ServiceState};
use crate::shared_kernel::{query_request::QueryRequestV1, query_response::QueryResponseV1};

pub(in crate::query) fn execute_query_v1(
    state: &ServiceState,
    request: &QueryRequestV1,
) -> Result<QueryResponseV1, ApiError> {
    let result = super::execute_query_inner(state, request);
    if result.as_ref().is_err() || result.as_ref().is_ok_and(|response| response.partial) {
        state.record_query_failure();
    }
    result
}
