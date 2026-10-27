//! Query HTTP extraction and rendering.
use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::query::application::get_query_job::query_job_for_project;
use crate::query::application::query_run::{run_query, tail_logs, QueryOutcome};
use crate::shared_kernel::query_job::QueryJobV1;
use crate::shared_kernel::query_phase_one::LogTailRequestV1;
use crate::shared_kernel::query_request::QueryRequestV1;
use crate::shared_kernel::query_response::QueryResponseV1;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use service_auth::RoleMapPrincipal;
use std::sync::Arc;

pub(crate) async fn query_v1(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    payload: Result<Json<QueryRequestV1>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) =
        payload.map_err(|error| ApiError::bad_request("invalid_json", error.body_text()))?;
    match run_query(state, principal.as_ref().map(|value| &value.0), request).await? {
        QueryOutcome::Queued(response) => {
            Ok((StatusCode::ACCEPTED, Json(response)).into_response())
        }
        QueryOutcome::Complete(response) => Ok(Json(response).into_response()),
    }
}
pub(crate) async fn tail_logs_v1(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    payload: Result<Json<LogTailRequestV1>, JsonRejection>,
) -> Result<Json<QueryResponseV1>, ApiError> {
    let Json(request) =
        payload.map_err(|error| ApiError::bad_request("invalid_json", error.body_text()))?;
    tail_logs(state, principal.as_ref().map(|value| &value.0), request)
        .await
        .map(Json)
}

#[derive(Deserialize)]
pub(crate) struct QueryJobHttpQuery {
    pub(super) project: String,
}

pub(crate) async fn get_query_job_v1(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<QueryJobHttpQuery>,
) -> Result<Json<QueryJobV1>, ApiError> {
    query_job_for_project(
        &state,
        principal.as_ref().map(|value| &value.0),
        &id,
        &query.project,
    )
    .map(Json)
}
