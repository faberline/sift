//! V2 HTTP extraction. Blocking scans run outside the async reactor.
use crate::app::{interfaces::http::api_error::ApiError, service_state::ServiceState};
use crate::query::application::query_v2::{QueryRequestV2, QueryResponseV2};
use axum::{
    extract::{rejection::JsonRejection, Extension, State},
    Json,
};
use service_auth::RoleMapPrincipal;
use std::sync::Arc;

pub(crate) async fn query_v2(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    payload: Result<Json<QueryRequestV2>, JsonRejection>,
) -> Result<Json<QueryResponseV2>, ApiError> {
    let Json(request) =
        payload.map_err(|e| ApiError::bad_request("invalid_json", e.body_text()))?;
    tokio::task::spawn_blocking(move || {
        state.execute_query_v2(principal.as_ref().map(|v| &v.0), &request)
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map(Json)
}
