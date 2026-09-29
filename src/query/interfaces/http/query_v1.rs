//! The versioned query endpoint, the log tail, and reading an async job back.

use std::sync::Arc;

use anyhow::Result;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use service_auth::RoleMapPrincipal;

use crate::query::application::execute_query::execute_query_v1;
use crate::query::application::get_query_job::query_job_for_project;
use crate::query::domain::query_cursor::encode_query_cursor;
use crate::query::domain::query_job::QueryJobV1;
use crate::query::interfaces::http::phase_one::LogTailRequestV1;
use crate::query::interfaces::http::query_request_v1::{
    QueryModeV1, QueryRequestV1, QuerySignalV1, TimeRangeV1,
};
use crate::query::interfaces::http::query_response_v1::{QueryResponseV1, QueryStatsV1};
use crate::{authorize_project_read, ApiError, ServiceState};

pub(crate) async fn query_v1(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    payload: Result<Json<QueryRequestV1>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(request) =
        payload.map_err(|error| ApiError::bad_request("invalid_json", error.body_text()))?;
    request
        .validate()
        .map_err(|error| ApiError::bad_request("invalid_query", error.to_string()))?;
    authorize_project_read(
        principal.as_ref().map(|principal| &principal.0),
        &request.project,
    )?;
    let asynchronous = request.mode == QueryModeV1::Async
        || (request.mode == QueryModeV1::Auto && request.limit > 500);
    if asynchronous {
        let job = state
            .query_jobs
            .create(request.clone())
            .map_err(|error| ApiError::internal(format!("create query job: {error}")))?;
        let query_id = job.query_id.clone();
        let worker_id = query_id.clone();
        let log_query_id = query_id.clone();
        let worker_state = state.clone();
        let runner = service_executor::JobRunner::new(state.query_jobs.clone());
        let task = runner.spawn_blocking(worker_id, job.request, move |request| {
            execute_query_v1(&worker_state, &request).map_err(|error| error.message)
        });
        tokio::spawn(async move {
            match task.await {
                Ok(report) if report.persistence_error.is_none() => {}
                Ok(report) => tracing::error!(
                    query_id = log_query_id,
                    error = report
                        .persistence_error
                        .as_deref()
                        .unwrap_or("unknown error"),
                    "persist query job transition failed"
                ),
                Err(error) => tracing::error!(
                    query_id = log_query_id,
                    %error,
                    "query job runner task failed"
                ),
            }
        });
        let response = QueryResponseV1 {
            data: serde_json::json!({"status": "queued"}),
            next_cursor: None,
            watermark: 0,
            partial: false,
            warnings: Vec::new(),
            stats: QueryStatsV1 {
                elapsed_ms: 0,
                scanned: 0,
                returned: 0,
            },
            query_id: Some(query_id),
        };
        return Ok((StatusCode::ACCEPTED, Json(response)).into_response());
    }
    let query_state = state.clone();
    let response = tokio::task::spawn_blocking(move || execute_query_v1(&query_state, &request))
        .await
        .map_err(|error| ApiError::internal(format!("query worker failed: {error}")))??;
    Ok(Json(response).into_response())
}

pub(crate) async fn tail_logs_v1(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    payload: Result<Json<LogTailRequestV1>, JsonRejection>,
) -> Result<Json<QueryResponseV1>, ApiError> {
    let Json(request) =
        payload.map_err(|error| ApiError::bad_request("invalid_json", error.body_text()))?;
    request
        .validate()
        .map_err(|error| ApiError::bad_request("invalid_tail_query", error.to_string()))?;
    authorize_project_read(
        principal.as_ref().map(|principal| &principal.0),
        &request.project,
    )?;
    let query = QueryRequestV1 {
        version: request.version,
        project: request.project,
        environment: request.environment,
        time_range: TimeRangeV1::default(),
        signal: QuerySignalV1::Logs {
            filter: request.filter,
        },
        limit: request.limit,
        cursor: request.after_cursor.clone(),
        mode: QueryModeV1::Sync,
    };
    query
        .validate()
        .map_err(|error| ApiError::bad_request("invalid_tail_query", error.to_string()))?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(request.wait_ms);
    loop {
        let mut response = execute_query_v1(&state, &query)?;
        let last_cursor = response.data["records"]
            .as_array()
            .and_then(|records| records.last())
            .and_then(|record| record["cursor"].as_u64());
        if let Some(last_cursor) = last_cursor {
            response.next_cursor = Some(encode_query_cursor("logs", &last_cursor.to_string()));
            return Ok(Json(response));
        }
        if tokio::time::Instant::now() >= deadline {
            response.next_cursor = request.after_cursor;
            return Ok(Json(response));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
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
    query_job_for_project(&state, principal.as_ref(), &id, &query.project).map(Json)
}
