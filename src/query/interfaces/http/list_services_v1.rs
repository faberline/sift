//! Service-listing HTTP extraction and rendering.
use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::shared_kernel::query_phase_one::{ServiceListResponseV1, ServiceQueryV1};
use axum::extract::{Extension, Query, State};
use axum::Json;
use service_auth::RoleMapPrincipal;
use std::sync::Arc;

pub(crate) async fn list_services_v1(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Query(query): Query<ServiceQueryV1>,
) -> Result<Json<ServiceListResponseV1>, ApiError> {
    crate::query::application::service_listing::list_services(
        &state,
        principal.as_ref().map(|value| &value.0),
        query,
    )
    .map(Json)
}
