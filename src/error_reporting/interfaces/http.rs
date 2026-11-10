//! Error status writes require project admin access.
use crate::access::application::project_authorization::authorize_project_admin;
use crate::app::{
    interfaces::http::{
        api_error::ApiError,
        control_records::{list, ListRequest, RecordPage},
    },
    ServiceState,
};
use crate::control::application::service::RecordKind;
use crate::error_reporting::application::service::ErrorReportingService;
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use service_auth::RoleMapPrincipal;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatusRequest {
    project: String,
    request_id: String,
    expected_revision: u64,
    status: String,
}
pub(crate) async fn error_groups(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Query(request): Query<ListRequest>,
) -> Result<Json<RecordPage>, ApiError> {
    list(
        &state,
        principal.as_ref().map(|p| &p.0),
        request,
        RecordKind::ErrorGroup,
    )
    .await
}
pub(crate) async fn set_status(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Path(id): Path<String>,
    Json(request): Json<StatusRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_project_admin(principal.as_ref().map(|p| &p.0), &request.project)?;
    if !["open", "resolved", "ignored"].contains(&request.status.as_str()) {
        return Err(ApiError::bad_request(
            "invalid_status",
            "status must be open, resolved, or ignored",
        ));
    }
    ErrorReportingService::new(state.control_service()?.clone())
        .status(
            &request.project,
            &id,
            request.expected_revision,
            request.request_id,
            &request.status,
        )
        .await
        .map_err(|e| {
            if e.contains("revision conflict") {
                ApiError::conflict("revision_conflict", e)
            } else {
                ApiError::temporarily_unavailable(e)
            }
        })?;
    Ok(Json(json!({"updated":true})))
}
