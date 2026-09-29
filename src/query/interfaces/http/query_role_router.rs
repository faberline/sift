//! The query role's router: versioned queries forwarded to the store, and async
//! jobs kept on the query role's own data root.

use std::sync::Arc;

use anyhow::Result;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use service_auth::RoleMapPrincipal;

use crate::access::interfaces::http::project_authorization::authorize_project_read;
use crate::operations::interfaces::http::gateway_proxy::query_router;
use crate::query::application::get_query_job::query_job_for_project;
use crate::query::domain::query_job::QueryJobV1;
use crate::query::infrastructure::store_query_client::{
    decode_store_query_response, forward_query_to_store,
};
use crate::query::interfaces::http::query_request_v1::{QueryModeV1, QueryRequestV1};
use crate::query::interfaces::http::query_response_v1::{QueryResponseV1, QueryStatsV1};
use crate::query::interfaces::http::query_v1::QueryJobHttpQuery;
use crate::{ApiError, ServiceState};

#[derive(Clone)]
struct QueryRoleState {
    service: Arc<ServiceState>,
    store: Router,
    max_body_bytes: usize,
}

/// Build the query role. Sync work uses the store as its source of truth.
/// Async job state stays on the query role's persistent data root.
pub fn query_role_router(
    state: Arc<ServiceState>,
    store_endpoint: &str,
    max_body_bytes: usize,
) -> Result<Router> {
    let store = query_router(store_endpoint, max_body_bytes)?;
    Ok(Router::new()
        .route("/api/v1/query", post(query_role_query_v1))
        .route("/api/v1/queries/{query_id}", get(get_query_role_job_v1))
        .fallback_service(store.clone())
        .with_state(Arc::new(QueryRoleState {
            service: state,
            store,
            max_body_bytes,
        })))
}

async fn query_role_query_v1(
    State(state): State<Arc<QueryRoleState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    headers: HeaderMap,
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
    if !asynchronous {
        return forward_query_to_store(state.store.clone(), headers, &request)
            .await
            .map_err(ApiError::internal);
    }

    let job = state
        .service
        .query_jobs
        .create(request.clone())
        .map_err(|error| ApiError::internal(format!("create query job: {error}")))?;
    let query_id = job.query_id.clone();
    let worker_id = query_id.clone();
    let log_query_id = query_id.clone();
    let store = state.store.clone();
    let max_body_bytes = state.max_body_bytes;
    let runner = service_executor::JobRunner::new(state.service.query_jobs.clone());
    let task = runner.spawn_async(worker_id, job.request, move |mut request| async move {
        request.mode = QueryModeV1::Sync;
        let response = forward_query_to_store(store, headers, &request).await?;
        decode_store_query_response(response, max_body_bytes).await
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
                "persist query role job transition failed"
            ),
            Err(error) => tracing::error!(
                query_id = log_query_id,
                %error,
                "query role job runner task failed"
            ),
        }
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(QueryResponseV1 {
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
        }),
    )
        .into_response())
}

async fn get_query_role_job_v1(
    State(state): State<Arc<QueryRoleState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<QueryJobHttpQuery>,
) -> Result<Json<QueryJobV1>, ApiError> {
    query_job_for_project(&state.service, principal.as_ref(), &id, &query.project).map(Json)
}
