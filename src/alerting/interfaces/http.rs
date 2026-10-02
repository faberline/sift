//! The caller can read rules and states. Only project admins can change them.
use crate::access::application::project_authorization::authorize_project_admin;
use crate::alerting::application::service::AlertingService;
use crate::app::{
    interfaces::http::{
        api_error::ApiError,
        control_records::{list, put, DeleteRequest, ListRequest, RecordPage, WriteRequest},
    },
    ServiceState,
};
use crate::control::application::service::{Change, ControlRecord, RecordKind};
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use service_auth::RoleMapPrincipal;
use std::sync::Arc;

pub(crate) async fn rules(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Query(request): Query<ListRequest>,
) -> Result<Json<RecordPage>, ApiError> {
    list(
        &state,
        principal.as_ref().map(|p| &p.0),
        request,
        RecordKind::AlertRule,
    )
    .await
}
pub(crate) async fn alerts(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Query(request): Query<ListRequest>,
) -> Result<Json<RecordPage>, ApiError> {
    list(
        &state,
        principal.as_ref().map(|p| &p.0),
        request,
        RecordKind::Alert,
    )
    .await
}
pub(crate) async fn put_rule(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Path(id): Path<String>,
    Json(request): Json<WriteRequest>,
) -> Result<Json<ControlRecord>, ApiError> {
    AlertingService::validate_rule(&request.value, &request.project)
        .map_err(|e| ApiError::bad_request("invalid_rule", e))?;
    if let Some(expression) = request
        .value
        .pointer("/source/expression")
        .and_then(Value::as_str)
    {
        state.validate_metric_expression(expression)?;
    }
    put(
        &state,
        principal.as_ref().map(|p| &p.0),
        id,
        request,
        RecordKind::AlertRule,
    )
    .await
}
pub(crate) async fn delete_rule(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Path(id): Path<String>,
    Json(request): Json<DeleteRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_project_admin(principal.as_ref().map(|p| &p.0), &request.project)?;
    state
        .control_service()?
        .transact(
            request.request_id,
            vec![Change {
                project: request.project,
                kind: RecordKind::AlertRule,
                id,
                expected_revision: request.expected_revision,
                value: None,
            }],
        )
        .await
        .map_err(control_error)?;
    Ok(Json(json!({"deleted":true})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MuteRequest {
    project: String,
    request_id: String,
    expected_revision: u64,
    until: Option<i64>,
}
pub(crate) async fn mute(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Path(id): Path<String>,
    Json(request): Json<MuteRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_project_admin(principal.as_ref().map(|p| &p.0), &request.project)?;
    AlertingService::new(state.control_service()?.clone())
        .mute(
            &request.project,
            id,
            request.expected_revision,
            request.request_id,
            request.until,
        )
        .await
        .map_err(control_error)?;
    Ok(Json(json!({"updated":true})))
}
fn control_error(error: String) -> ApiError {
    if error.contains("revision conflict") {
        ApiError::conflict("revision_conflict", error)
    } else {
        ApiError::temporarily_unavailable(error)
    }
}
