//! Looking up one trace by id.

use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Extension, Path as AxumPath, Query, State};
use axum::Json;
use serde::Deserialize;
use service_auth::RoleMapPrincipal;

use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::projection;

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
    crate::query::application::trace_lookup::get_trace(
        &state,
        principal.as_ref().map(|value| &value.0),
        id,
        query.project,
        query.min_cursor,
    )
    .await
    .map(Json)
}
