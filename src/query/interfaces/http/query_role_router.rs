//! Query-role HTTP extraction, response rendering and injected fallback.

use crate::app::interfaces::http::api_error::ApiError;
use crate::query::application::query_role_service::{QueryRoleService, RoleQueryOutcome};
use crate::query::interfaces::http::forward_response::render_forward_response;
use crate::query::interfaces::http::query_v1::QueryJobHttpQuery;
use crate::shared_kernel::query_job::QueryJobV1;
use crate::shared_kernel::query_request::QueryRequestV1;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use service_auth::RoleMapPrincipal;
use std::sync::Arc;

pub(crate) fn build_query_role_router(service: Arc<QueryRoleService>, fallback: Router) -> Router {
    Router::new()
        .route("/api/v1/query", post(query_role_query_v1))
        .route("/api/v1/queries/{query_id}", get(get_query_role_job_v1))
        .fallback_service(fallback)
        .with_state(service)
}

async fn query_role_query_v1(
    State(state): State<Arc<QueryRoleService>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    headers: HeaderMap,
    payload: Result<Json<QueryRequestV1>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) =
        payload.map_err(|error| ApiError::bad_request("invalid_json", error.body_text()))?;
    let headers = headers
        .iter()
        .map(|(name, value)| (name.as_str().to_string(), value.as_bytes().to_vec()))
        .collect();
    match state
        .query(principal.as_ref().map(|value| &value.0), headers, request)
        .await?
    {
        RoleQueryOutcome::Forwarded(response) => render_forward_response(response),
        RoleQueryOutcome::Queued(response) => {
            Ok((StatusCode::ACCEPTED, Json(response)).into_response())
        }
    }
}

async fn get_query_role_job_v1(
    State(state): State<Arc<QueryRoleService>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<QueryJobHttpQuery>,
) -> Result<Json<QueryJobV1>, ApiError> {
    state
        .job(
            principal.as_ref().map(|value| &value.0),
            &id,
            &query.project,
        )
        .map(Json)
}
