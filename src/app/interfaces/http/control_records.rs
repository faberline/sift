//! Common project checks and pagination for the control APIs.
use crate::access::application::project_authorization::{
    authorize_project_admin, authorize_project_read,
};
use crate::app::{interfaces::http::api_error::ApiError, service_state::ServiceState};
use crate::control::application::service::{Change, ControlRecord, RecordKind};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use service_auth::RoleMapPrincipal;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListRequest {
    pub project: String,
    pub after: Option<String>,
    pub limit: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WriteRequest {
    pub project: String,
    pub request_id: String,
    pub expected_revision: u64,
    pub value: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeleteRequest {
    pub project: String,
    pub request_id: String,
    pub expected_revision: u64,
}
#[derive(Serialize)]
pub(crate) struct RecordPage {
    pub records: Vec<ControlRecord>,
    pub next_cursor: Option<String>,
}

pub(crate) async fn list(
    state: &ServiceState,
    principal: Option<&RoleMapPrincipal>,
    request: ListRequest,
    kind: RecordKind,
) -> Result<Json<RecordPage>, ApiError> {
    authorize_project_read(principal, &request.project)?;
    let limit = request.limit.unwrap_or(100);
    if limit == 0 || limit > 1000 {
        return Err(ApiError::bad_request(
            "invalid_query",
            "control limit must be between 1 and 1000",
        ));
    }
    let mut records = state
        .control_service()?
        .list(&request.project, kind)
        .await
        .map_err(ApiError::temporarily_unavailable)?;
    records
        .retain(|r| r.value.is_some() && request.after.as_ref().is_none_or(|after| r.id > *after));
    records.sort_by(|a, b| a.id.cmp(&b.id));
    let more = records.len() > limit;
    records.truncate(limit);
    let next_cursor = more.then(|| records.last().expect("nonempty control page").id.clone());
    Ok(Json(RecordPage {
        records,
        next_cursor,
    }))
}

pub(crate) async fn put(
    state: &ServiceState,
    principal: Option<&RoleMapPrincipal>,
    id: String,
    request: WriteRequest,
    kind: RecordKind,
) -> Result<Json<ControlRecord>, ApiError> {
    authorize_project_admin(principal, &request.project)?;
    let change = Change {
        project: request.project.clone(),
        kind,
        id: id.clone(),
        expected_revision: request.expected_revision,
        value: Some(request.value),
    };
    change
        .validate()
        .map_err(|e| ApiError::bad_request("invalid_record", e))?;
    state
        .control_service()?
        .transact(request.request_id, vec![change])
        .await
        .map_err(|e| {
            if e.contains("revision conflict") {
                ApiError::conflict("revision_conflict", e)
            } else {
                ApiError::temporarily_unavailable(e)
            }
        })?;
    let record = state
        .control_service()?
        .list(&request.project, kind)
        .await
        .map_err(ApiError::temporarily_unavailable)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| ApiError::internal("committed record is missing"))?;
    Ok(Json(record))
}
