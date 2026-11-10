//! Trace lookup with the original cursor wait and error mapping.

use crate::access::application::project_authorization::authorize_project_read;
use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::projection;
use anyhow::Result;
use service_auth::RoleMapPrincipal;

const LOG_QUERY_PROJECTION_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

pub(crate) async fn get_trace(
    state: &ServiceState,
    principal: Option<&RoleMapPrincipal>,
    id: String,
    project: String,
    min_cursor: Option<u64>,
) -> Result<projection::TraceResultV1, ApiError> {
    authorize_project_read(principal, &project)?;
    if id.trim().is_empty() {
        return Err(ApiError::bad_request(
            "invalid_trace_id",
            "trace id must not be empty",
        ));
    }
    let catching_up = state.clone();
    tokio::task::spawn_blocking(move || {
        catching_up
            .journal()
            .ensure_queryable()
            .map_err(|error| ApiError::temporarily_unavailable(error.to_string()))?;
        catching_up
            .projections()
            .catch_up(projection::PROJECTION_TRACE_STORE)
            .map_err(|error| ApiError::internal(error.to_string()))
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;
    let projection_cursor = state
        .projections()
        .wait_for_min_cursor(
            projection::PROJECTION_TRACE_STORE,
            min_cursor.unwrap_or(0),
            LOG_QUERY_PROJECTION_WAIT,
        )
        .await
        .map_err(ApiError::projection_lag)?;
    let reading = state.clone();
    let lookup_project = project.clone();
    let lookup_id = id.clone();
    let mut trace = tokio::task::spawn_blocking(move || {
        reading.projections().get_trace(&lookup_project, &lookup_id)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))?
    .map_err(|error| ApiError::bad_request("invalid_trace_query", error.to_string()))?
    .ok_or_else(|| {
        ApiError::not_found(
            "trace_not_found",
            format!("trace `{id}` was not found in project `{}`", project),
        )
    })?;
    trace.projection_cursor = projection_cursor;
    Ok(trace)
}
