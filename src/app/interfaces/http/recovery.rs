//! Capture a complete archive prefix for an authorized project admin.
use crate::access::application::project_authorization::authorize_project_admin;
use crate::app::{interfaces::http::api_error::ApiError, ServiceState};
use crate::shared_kernel::recovery_record::DataRecovery;
use axum::{
    extract::{Extension, State},
    Json,
};
use serde::Deserialize;
use service_auth::RoleMapPrincipal;
use std::sync::Arc;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaptureRequest {
    project: String,
}
pub(crate) async fn capture(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Json(request): Json<CaptureRequest>,
) -> Result<Json<DataRecovery>, ApiError> {
    authorize_project_admin(principal.as_ref().map(|p| &p.0), &request.project)?;
    state
        .capture_recovery_data()
        .await
        .map(Json)
        .map_err(|e| ApiError::temporarily_unavailable(e.to_string()))
}
