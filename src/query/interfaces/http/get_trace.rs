//! Looking up one trace by id.

use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Extension, Path as AxumPath, Query, State};
use axum::Json;
use serde::Deserialize;
use service_auth::RoleMapPrincipal;

use crate::access::interfaces::http::project_authorization::authorize_project_read;
use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::projection;

const LOG_QUERY_PROJECTION_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

#[derive(Debug, Deserialize)]
pub(crate) struct HttpTraceQuery {
    project: String,
    min_cursor: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/v1/traces/{id}",
    params(
        ("id" = String, Path, description = "trace id"),
        ("project" = String, Query, description = "authorized project"),
        ("min_cursor" = Option<u64>, Query, description = "read-your-write projection cursor")
    ),
    responses(
        (status = 200, description = "complete or explicitly partial trace", body = projection::TraceResultV1),
        (status = 400, description = "invalid trace query", body = ErrorEnvelope),
        (status = 403, description = "project read denied", body = ErrorEnvelope),
        (status = 404, description = "trace not found", body = ErrorEnvelope),
        (status = 503, description = "projection has not reached min_cursor", body = ErrorEnvelope)
    )
)]
pub(crate) async fn get_trace(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<HttpTraceQuery>,
) -> Result<Json<projection::TraceResultV1>, ApiError> {
    authorize_project_read(principal.as_ref().map(|value| &value.0), &query.project)?;
    if id.trim().is_empty() {
        return Err(ApiError::bad_request(
            "invalid_trace_id",
            "trace id must not be empty",
        ));
    }
    state
        .journal
        .ensure_queryable()
        .map_err(|error| ApiError::temporarily_unavailable(error.to_string()))?;
    state
        .projections
        .catch_up(projection::PROJECTION_TRACE_STORE)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let projection_cursor = state
        .projections
        .wait_for_min_cursor(
            projection::PROJECTION_TRACE_STORE,
            query.min_cursor.unwrap_or(0),
            LOG_QUERY_PROJECTION_WAIT,
        )
        .await
        .map_err(ApiError::projection_lag)?;
    let mut trace = state
        .projections
        .get_trace(&query.project, &id)
        .map_err(|error| ApiError::bad_request("invalid_trace_query", error.to_string()))?
        .ok_or_else(|| {
            ApiError::not_found(
                "trace_not_found",
                format!("trace `{id}` was not found in project `{}`", query.project),
            )
        })?;
    trace.projection_cursor = projection_cursor;
    Ok(Json(trace))
}
