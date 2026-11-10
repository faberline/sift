//! Saved queries share the project admin write contract.
use crate::access::application::project_authorization::authorize_project_admin;
use crate::app::{
    interfaces::http::{
        api_error::ApiError,
        control_records::{list, put, DeleteRequest, ListRequest, RecordPage, WriteRequest},
    },
    service_state::ServiceState,
};
use crate::control::application::service::{Change, ControlRecord, RecordKind};
use crate::query::application::query_v2::QueryRequestV2;
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde_json::Value;
use service_auth::RoleMapPrincipal;
use std::sync::Arc;

pub(crate) async fn saved_queries(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Query(request): Query<ListRequest>,
) -> Result<Json<RecordPage>, ApiError> {
    list(
        &state,
        principal.as_ref().map(|p| &p.0),
        request,
        RecordKind::SavedQuery,
    )
    .await
}
pub(crate) async fn put_saved_query(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Path(id): Path<String>,
    Json(request): Json<WriteRequest>,
) -> Result<Json<ControlRecord>, ApiError> {
    let saved: QueryRequestV2 =
        serde_json::from_value(request.value.get("request").cloned().ok_or_else(|| {
            ApiError::bad_request("invalid_record", "saved query requires request")
        })?)
        .map_err(|e| ApiError::bad_request("invalid_record", e.to_string()))?;
    if saved.project != request.project || saved.version != 2 {
        return Err(ApiError::bad_request(
            "invalid_record",
            "saved query project and version must match",
        ));
    }
    if let crate::shared_kernel::query_v2::QuerySignalV2::Metrics { expression } = &saved.signal {
        state.validate_metric_expression(expression)?;
    }
    put(
        &state,
        principal.as_ref().map(|p| &p.0),
        id,
        request,
        RecordKind::SavedQuery,
    )
    .await
}
pub(crate) async fn delete_saved_query(
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
                kind: RecordKind::SavedQuery,
                id,
                expected_revision: request.expected_revision,
                value: None,
            }],
        )
        .await
        .map_err(ApiError::temporarily_unavailable)?;
    Ok(Json(serde_json::json!({"deleted":true})))
}
