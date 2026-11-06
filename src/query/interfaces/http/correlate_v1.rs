//! Correlation HTTP extraction and rendering.
use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::shared_kernel::query_phase_one::{CorrelationRequestV1, CorrelationResponseV1};
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, State};
use axum::Json;
use service_auth::RoleMapPrincipal;
use std::sync::Arc;

pub(crate) async fn correlate_v1(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    payload: Result<Json<CorrelationRequestV1>, JsonRejection>,
) -> Result<Json<CorrelationResponseV1>, ApiError> {
    let Json(request) =
        payload.map_err(|error| ApiError::bad_request("invalid_json", error.body_text()))?;
    crate::query::application::correlation::correlate(
        &state,
        principal.as_ref().map(|value| &value.0),
        request,
    )
    .map(Json)
}
