//! Sift's data-plane routes, alone or behind the scoped bearer middleware, with
//! or without the MCP Streamable HTTP endpoint.

use std::sync::Arc;

use anyhow::Result;
use axum::routing::{get, post};
use axum::Router;

use crate::access::infrastructure::sift_verifier::SiftVerifier;
use crate::access::interfaces::http::scoped_authorization::auth_middleware;
use crate::app::service_state::ServiceState;
use crate::ingest::interfaces::http::otlp_handlers::{ingest_logs, ingest_metrics, ingest_traces};
use crate::ingest::interfaces::http::prometheus_remote_write::prometheus_remote_write;
use crate::operations::interfaces::http::admin_backup::admin_backup;
use crate::operations::interfaces::http::admin_integrity::admin_integrity;
use crate::query::interfaces::http::correlate_v1::correlate_v1;
use crate::query::interfaces::http::get_trace::get_trace;
use crate::query::interfaces::http::list_services_v1::list_services_v1;
use crate::query::interfaces::http::prometheus_query::{
    prometheus_instant_query, prometheus_range_query,
};
use crate::query::interfaces::http::query_v1::{get_query_job_v1, query_v1, tail_logs_v1};
use crate::query::interfaces::mcp::mcp_transport::http_router;

/// Build Sift's data-plane routes. Probe/admin routes are intentionally added
/// by `service-http` so all k8s-native services have the same shape.
pub fn router(state: Arc<ServiceState>) -> Router {
    Router::new()
        .route("/api/v1/query", post(query_v1))
        .route("/api/v1/logs/tail", post(tail_logs_v1))
        .route("/api/v1/traces/{trace_id}", get(get_trace))
        .route("/api/v1/correlate", post(correlate_v1))
        .route("/api/v1/services", get(list_services_v1))
        .route("/api/v1/queries/{query_id}", get(get_query_job_v1))
        .route("/prometheus/api/v1/write", post(prometheus_remote_write))
        .route(
            "/prometheus/api/v1/query",
            get(prometheus_instant_query).post(prometheus_instant_query),
        )
        .route(
            "/prometheus/api/v1/query_range",
            get(prometheus_range_query).post(prometheus_range_query),
        )
        .route("/v1/logs", post(ingest_logs))
        .route("/v1/traces", post(ingest_traces))
        .route("/v1/metrics", post(ingest_metrics))
        .route("/admin/backup", get(admin_backup))
        .route("/admin/integrity", get(admin_integrity))
        .with_state(state)
}

/// Build the production data-plane router. The standard operational probe
/// router is intentionally composed outside this function, so its endpoints
/// remain reachable when `SIFT_AUTH=required`.
pub fn protected_router(state: Arc<ServiceState>, verifier: Arc<SiftVerifier>) -> Router {
    router(state).layer(axum::middleware::from_fn_with_state(
        verifier,
        auth_middleware,
    ))
}

/// Build the protected HTTP data plane plus the official MCP Streamable HTTP
/// endpoint. MCP tools forward the caller's credential to these same routes.
pub fn protected_router_with_mcp(
    state: Arc<ServiceState>,
    verifier: Arc<SiftVerifier>,
    internal_endpoint: &str,
) -> Result<Router> {
    Ok(router(state).merge(http_router(internal_endpoint)?).layer(
        axum::middleware::from_fn_with_state(verifier, auth_middleware),
    ))
}
