//! Reading an async query job back for the project that asked for it.

use anyhow::Result;
use axum::extract::Extension;
use service_auth::RoleMapPrincipal;

use crate::access::interfaces::http::project_authorization::authorize_project_read;
use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::query::domain::query_job::QueryJobV1;

pub(in crate::query) fn query_job_for_project(
    state: &ServiceState,
    principal: Option<&Extension<RoleMapPrincipal>>,
    id: &str,
    project: &str,
) -> Result<QueryJobV1, ApiError> {
    authorize_project_read(principal.map(|principal| &principal.0), project)?;
    let job = state
        .query_jobs
        .get(id)
        .map_err(|error| ApiError::bad_request("invalid_query_id", error.to_string()))?
        .filter(|job| job.project == project)
        .ok_or_else(|| {
            ApiError::not_found(
                "query_not_found",
                format!("query `{id}` was not found in project `{project}`"),
            )
        })?;
    Ok(job)
}
