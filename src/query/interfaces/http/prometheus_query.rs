//! Prometheus HTTP extraction and rendering.
use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::shared_kernel::prom_query_params::{InstantQueryParams, RangeQueryParams};
use axum::extract::{Extension, Query, State};
use axum::Json;
use service_auth::RoleMapPrincipal;
use std::sync::Arc;

pub(crate) async fn prometheus_instant_query(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Query(params): Query<InstantQueryParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::query::application::prometheus_queries::prometheus_instant_query(
        &state,
        principal.as_ref().map(|value| &value.0),
        params,
    )
    .map(Json)
}
pub(crate) async fn prometheus_range_query(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Query(params): Query<RangeQueryParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    crate::query::application::prometheus_queries::prometheus_range_query(
        &state,
        principal.as_ref().map(|value| &value.0),
        params,
    )
    .map(Json)
}
