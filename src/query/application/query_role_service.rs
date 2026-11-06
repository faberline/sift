//! Query-role decisions use job storage and forwarding application services.

use crate::access::application::project_authorization::authorize_project_read;
use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::query::application::forward_service::{ForwardResponse, QueryForwardService};
use crate::query::application::get_query_job::query_job_for_project;
use crate::shared_kernel::forward_data::ForwardHeaders;
use crate::shared_kernel::query_job::QueryJobV1;
use crate::shared_kernel::query_request::{QueryModeV1, QueryRequestV1};
use crate::shared_kernel::query_response::{QueryResponseV1, QueryStatsV1};
use service_auth::RoleMapPrincipal;
use std::sync::Arc;

pub(crate) enum RoleQueryOutcome {
    Forwarded(ForwardResponse),
    Queued(QueryResponseV1),
}

pub(crate) struct QueryRoleService {
    service: Arc<ServiceState>,
    forwarding: Arc<QueryForwardService>,
}

impl QueryRoleService {
    pub(crate) fn new(service: Arc<ServiceState>, forwarding: Arc<QueryForwardService>) -> Self {
        Self {
            service,
            forwarding,
        }
    }
    pub(crate) async fn query(
        &self,
        principal: Option<&RoleMapPrincipal>,
        headers: ForwardHeaders,
        request: QueryRequestV1,
    ) -> Result<RoleQueryOutcome, ApiError> {
        request
            .validate()
            .map_err(|error| ApiError::bad_request("invalid_query", error.to_string()))?;
        authorize_project_read(principal, &request.project)?;
        let asynchronous = request.mode == QueryModeV1::Async
            || (request.mode == QueryModeV1::Auto && request.limit > 500);
        if !asynchronous {
            return self
                .forwarding
                .forward(headers, &request)
                .await
                .map(RoleQueryOutcome::Forwarded)
                .map_err(ApiError::internal);
        }

        let job = self
            .service
            .query_jobs()
            .create(request.clone())
            .map_err(|error| ApiError::internal(format!("create query job: {error}")))?;
        let query_id = job.query_id.clone();
        let worker_id = query_id.clone();
        let log_query_id = query_id.clone();
        let forwarding = self.forwarding.clone();
        let runner = service_executor::JobRunner::new(self.service.query_jobs().clone());
        let task = runner.spawn_async(worker_id, job.request, move |mut request| async move {
            request.mode = QueryModeV1::Sync;
            let response = forwarding.forward(headers, &request).await?;
            forwarding.decode(response).await
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

        Ok(RoleQueryOutcome::Queued(QueryResponseV1 {
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
        }))
    }
    pub(crate) fn job(
        &self,
        principal: Option<&RoleMapPrincipal>,
        id: &str,
        project: &str,
    ) -> Result<QueryJobV1, ApiError> {
        query_job_for_project(&self.service, principal, id, project)
    }
}
