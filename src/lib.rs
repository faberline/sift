//! Sift's service core for logs, metrics, and traces. The canonical per-signal
//! WAL is fsynced before acknowledgement. Rebuildable indexes are never a
//! second source of truth.

mod access;
pub mod api;
mod archive;
pub mod auth;
pub mod backup;
pub mod collector;
pub mod deploy;
pub mod durability;
pub mod event;
pub mod grpc;
pub mod ingest;
mod journal;
pub mod mcp;
mod node;
mod operations;
pub mod operator;
pub mod projection;
pub mod prometheus;
pub mod proxy;
mod query;
mod shared_kernel;
pub mod storage;

pub use crate::archive::interfaces::archive_worker::ArchiveWorker;
pub use crate::ingest::domain::governance_policy::{GovernancePolicy, GovernancePolicySet};
pub use crate::journal::domain::append_result::AppendResult;
pub use crate::journal::domain::event_query::EventQuery;
pub use crate::journal::infrastructure::durable_journal::DurableJournal;
pub use crate::journal::infrastructure::journal_projection_read_session::JournalProjectionReadSession;
pub use crate::operations::interfaces::http::integrity_report_v1::{
    IntegrityArchiveV1, IntegrityReportV1, IntegritySignalV1, IntegritySignalsV1,
    IntegrityStorageV1, IntegrityWalBytesV1, IntegrityWatermarksV1,
};
pub use crate::projection::interfaces::projection_worker::ProjectionWorker;
pub use crate::query::interfaces::http::query_role_router::query_role_router;
pub use crate::shared_kernel::stored_event::StoredEvent;
pub use event::{
    decode_event_json, AttributeValue, ContentBlobRef, EventEnvelope, IncomingEvent,
    InstrumentationScope, MetricExemplar, MetricPoint, MetricTemporality, OperationalEventV2,
    SignalKind, EVENT_SCHEMA_URL, EVENT_SCHEMA_VERSION,
};

use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::{Context, Result};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use chrono::Utc;
use metrics_prometheus::Sample;
use service_http::{DetailedErrorEnvelope as ErrorEnvelope, ProjectionMetadata};
use utoipa::OpenApi;

use crate::access::infrastructure::sift_verifier::SiftVerifier;
use crate::access::interfaces::http::scoped_authorization::auth_middleware;
use crate::ingest::application::admission_controller::AdmissionController;
use crate::ingest::domain::admission_error::AdmissionError;
use crate::ingest::domain::ingest_limits::IngestLimits;
use crate::ingest::infrastructure::ingest_batch_coordinator::IngestBatchCoordinator;
use crate::ingest::interfaces::http::otlp_handlers::{
    __path_ingest_logs, __path_ingest_metrics, __path_ingest_traces, ingest_logs, ingest_metrics,
    ingest_traces,
};
use crate::ingest::interfaces::http::prometheus_remote_write::prometheus_remote_write;
use crate::journal::infrastructure::raft::sift_membership_policy::SiftMembershipPolicy;
use crate::node::domain::storage_role::StorageRole;
use crate::node::infrastructure::local_capacity::LocalCapacity;
use crate::operations::interfaces::http::admin_backup::{__path_admin_backup, admin_backup};
use crate::operations::interfaces::http::admin_integrity::{
    __path_admin_integrity, admin_integrity,
};
use crate::query::infrastructure::file_query_job_store::QueryJobStore;
use crate::query::interfaces::http::correlate_v1::correlate_v1;
use crate::query::interfaces::http::get_trace::get_trace;
use crate::query::interfaces::http::list_services_v1::list_services_v1;
use crate::query::interfaces::http::prometheus_query::{
    prometheus_instant_query, prometheus_range_query,
};
use crate::query::interfaces::http::query_v1::{get_query_job_v1, query_v1, tail_logs_v1};
use crate::query::interfaces::mcp::mcp_transport::http_router;
use crate::shared_kernel::retention_boundary::retention_rejection_at;

/// Shared HTTP state: journal access plus the drain bit read by `/readyz`.
#[derive(Clone)]
pub struct ServiceState {
    journal: Arc<DurableJournal>,
    draining: Arc<AtomicBool>,
    raft: Option<Arc<raft_runtime::RaftHost>>,
    peer_transport: Option<raft_runtime::PeerTransport>,
    peer_port: Option<u16>,
    state_machine: Arc<crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine>,
    local_command: Arc<tokio::sync::Mutex<()>>,
    projections: Arc<projection::ProjectionRuntime>,
    admission: Arc<AdmissionController>,
    local_capacity: Arc<LocalCapacity>,
    query_jobs: Arc<QueryJobStore>,
    batch_coordinator: Arc<std::sync::Mutex<IngestBatchCoordinator>>,
}

impl ServiceState {
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_role(data_dir, StorageRole::All)
    }

    pub fn open_with_role(data_dir: impl AsRef<Path>, role: StorageRole) -> Result<Self> {
        Self::open_with_ingest_limits_and_role(data_dir, IngestLimits::from_env()?, role)
    }

    pub fn open_with_ingest_limits(
        data_dir: impl AsRef<Path>,
        limits: IngestLimits,
    ) -> Result<Self> {
        Self::open_with_ingest_limits_and_role(data_dir, limits, StorageRole::All)
    }

    pub fn open_with_ingest_limits_and_role(
        data_dir: impl AsRef<Path>,
        limits: IngestLimits,
        role: StorageRole,
    ) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        let journal = Arc::new(DurableJournal::open_with_role(data_dir, role)?);
        let local_capacity = Arc::new(LocalCapacity::open(
            data_dir,
            limits.max_local_storage_bytes,
            limits.min_local_free_bytes,
        )?);
        let state_machine = Arc::new(
            crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine::open(
                data_dir,
                journal.clone(),
            )?,
        );
        let (raft, peer_transport, peer_port) = if raft_runtime::replica_mode() {
            let peer_port = std::env::var("SIFT_PEER_PORT")
                .unwrap_or_else(|_| "7381".to_string())
                .parse::<u16>()
                .context("SIFT_PEER_PORT must be a valid TCP port")?;
            let headless =
                std::env::var("SIFT_RAFT_HEADLESS").unwrap_or_else(|_| "sift-peer".to_string());
            let runtime = raft_runtime::ReplicaHostBuilder::new(
                "sift",
                headless,
                peer_port,
                "SIFT_PEERS",
                "https",
                SiftMembershipPolicy,
            )?
            .build_secure(
                data_dir,
                state_machine.clone(),
                "SIFT_PEER",
                raft_runtime::FsyncPolicy::Always,
                raft_runtime::HostConfig::default(),
            )
            .context("replicated Sift requires peer mTLS")?;
            (
                Some(runtime.host),
                Some(runtime.peer_transport),
                Some(runtime.peer_port),
            )
        } else {
            (None, None, None)
        };
        Ok(Self {
            projections: Arc::new(projection::ProjectionRuntime::open(
                data_dir,
                journal.clone(),
            )?),
            journal,
            draining: Arc::new(AtomicBool::new(false)),
            raft,
            peer_transport,
            peer_port,
            state_machine,
            local_command: Arc::new(tokio::sync::Mutex::new(())),
            admission: Arc::new(AdmissionController::new(limits)?),
            local_capacity,
            query_jobs: Arc::new(QueryJobStore::open(data_dir.join("query-jobs"))?),
            batch_coordinator: Arc::new(std::sync::Mutex::new(IngestBatchCoordinator::default())),
        })
    }

    pub fn journal(&self) -> &DurableJournal {
        &self.journal
    }

    pub fn projections(&self) -> &projection::ProjectionRuntime {
        &self.projections
    }

    /// Start the one in-process projection worker owned by the Sift service.
    /// The worker has no listener, WAL, or Raft group of its own and can be
    /// stopped after HTTP drain during graceful shutdown.
    pub fn start_projection_worker(&self) -> ProjectionWorker {
        let projections = self.projections.clone();
        let flush_projections = self.projections.clone();
        let journal = self.journal.clone();
        let flush_journal = self.journal.clone();
        let (shutdown, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                        let runtime = projections.clone();
                        let journal = journal.clone();
                        match tokio::task::spawn_blocking(move || {
                            journal.maintain_dedupe_at(Utc::now(), false)?;
                            journal.storage().seal_ready()?;
                            for name in runtime.projection_names() {
                                runtime.catch_up(&name)?;
                            }
                            anyhow::Ok(())
                        }).await {
                            Ok(Ok(_)) => {}
                            Ok(Err(error)) => tracing::warn!(%error, "projection worker catch-up failed"),
                            Err(error) => tracing::warn!(%error, "projection worker task panicked"),
                        }
                    }
                }
            }
        });
        ProjectionWorker {
            shutdown: Some(shutdown),
            task,
            projections: flush_projections,
            journal: flush_journal,
        }
    }
}

impl service_http::ReadinessHook for ServiceState {
    fn is_draining(&self) -> bool {
        self.draining.load(Ordering::Acquire)
            || self.journal.recovery_required()
            || self.local_capacity.level() == storage::CapacityLevel::Critical
    }
}

impl service_http::MetricsProvider for ServiceState {
    fn render_metrics(&self) -> String {
        let mut text = self.journal.metrics_text();
        text.push_str(&metrics_prometheus::render(&[
            Sample::new(
                "sift_local_storage_used_bytes",
                "gauge",
                "Reserved bytes in the local Sift data root.",
                self.local_capacity.used_bytes(),
            ),
            Sample::new(
                "sift_local_storage_max_bytes",
                "gauge",
                "Configured local Sift storage safety capacity.",
                self.local_capacity.max_bytes(),
            ),
            Sample::new(
                "sift_local_storage_warning",
                "gauge",
                "One when local Sift storage is at or above the 70 percent warning threshold.",
                u64::from(matches!(
                    self.local_capacity.level(),
                    storage::CapacityLevel::Warning
                        | storage::CapacityLevel::Backpressure
                        | storage::CapacityLevel::Critical
                )),
            ),
            Sample::new(
                "sift_local_storage_critical",
                "gauge",
                "One when local Sift storage is at or above the 90 percent readiness threshold.",
                u64::from(self.local_capacity.level() == storage::CapacityLevel::Critical),
            ),
        ]));
        text
    }
}

struct ApiError {
    status: StatusCode,
    error: &'static str,
    message: String,
    retryable: bool,
    retry_after_secs: Option<u64>,
    projection_lag: Option<Box<projection::ProjectionLag>>,
}

impl ApiError {
    fn bad_request(error: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error,
            message: message.into(),
            retryable: false,
            retry_after_secs: None,
            projection_lag: None,
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: "journal_failure",
            message: message.into(),
            retryable: true,
            retry_after_secs: Some(1),
            projection_lag: None,
        }
    }

    fn temporarily_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error: "retention_checkpoint_pending",
            message: message.into(),
            retryable: true,
            retry_after_secs: Some(1),
            projection_lag: None,
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            error: "project_forbidden",
            message: message.into(),
            retryable: false,
            retry_after_secs: None,
            projection_lag: None,
        }
    }

    fn not_found(error: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            error,
            message: message.into(),
            retryable: false,
            retry_after_secs: None,
            projection_lag: None,
        }
    }

    fn unsupported_media(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            error: "unsupported_media_type",
            message: message.into(),
            retryable: false,
            retry_after_secs: None,
            projection_lag: None,
        }
    }

    fn from_admission(error: AdmissionError) -> Self {
        Self {
            status: error.status,
            error: error.code,
            message: error.message,
            retryable: error.retryable,
            retry_after_secs: error.retry_after_secs,
            projection_lag: None,
        }
    }

    fn projection_lag(lag: projection::ProjectionLag) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            error: "projection_lag",
            message: format!(
                "projection `{}` is at cursor {}, below required cursor {}",
                lag.projection, lag.current_cursor, lag.required_cursor
            ),
            retryable: true,
            retry_after_secs: Some(lag.retry_after_seconds),
            projection_lag: Some(Box::new(lag)),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let lag = self.projection_lag;
        let message = if self.retryable {
            format!("{} (retryable)", self.message)
        } else {
            self.message
        };
        let mut error = service_http::ApiErr::new(self.status, self.error, message)
            .with_retryable(self.retryable);
        if let Some(seconds) = self.retry_after_secs {
            error = error.with_retry_after_seconds(seconds);
        }
        if let Some(lag) = lag {
            error = error.with_projection(ProjectionMetadata {
                projection: lag.projection,
                required_cursor: lag.required_cursor,
                current_cursor: lag.current_cursor,
            });
        }
        error.into_response()
    }
}

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

#[derive(OpenApi)]
#[openapi(
    paths(
        ingest_logs,
        ingest_traces,
        ingest_metrics,
        admin_backup,
        admin_integrity
    ),
    components(schemas(
        AttributeValue,
        InstrumentationScope,
        MetricPoint,
        MetricTemporality,
        MetricExemplar,
        projection::LogRecordV1,
        projection::SpanLinkV1,
        projection::SpanEventV1,
        projection::SpanRecordV1,
        projection::TraceResultV1,
        projection::HistogramKind,
        projection::MetricHistogramV1,
        projection::MetricPointV1,
        projection::MetricChunkV1,
        projection::MetricRollupV1,
        projection::MetricAggregation,
        projection::MetricSeriesResultV1,
        IntegritySignalV1,
        IntegritySignalsV1,
        IntegrityWatermarksV1,
        IntegrityWalBytesV1,
        IntegrityArchiveV1,
        IntegrityStorageV1,
        IntegrityReportV1,
        ErrorEnvelope
    )),
    tags((name = "telemetry", description = "Sift logs, metrics, and traces"))
)]
struct SiftApi;

pub fn openapi() -> utoipa::openapi::OpenApi {
    use utoipa::openapi::{
        path::{OperationBuilder, PathItem, PathItemType},
        response::ResponseBuilder,
    };

    let mut document = SiftApi::openapi();
    for (path, method, summary) in [
        (
            "/api/v1/query",
            "post",
            "Run one versioned logs, metrics, or traces query",
        ),
        (
            "/api/v1/logs/tail",
            "post",
            "Read a bounded resumable log tail",
        ),
        ("/api/v1/traces/{trace_id}", "get", "Read one trace"),
        ("/api/v1/correlate", "post", "Find related telemetry"),
        ("/api/v1/services", "get", "List observed services"),
        (
            "/api/v1/queries/{query_id}",
            "get",
            "Read a persistent asynchronous query job",
        ),
        (
            "/prometheus/api/v1/write",
            "post",
            "Receive Prometheus Remote Write 1.0",
        ),
        (
            "/prometheus/api/v1/query",
            "get",
            "Run an instant PromQL query",
        ),
        (
            "/prometheus/api/v1/query_range",
            "get",
            "Run a range PromQL query",
        ),
    ] {
        let method = match method {
            "get" => PathItemType::Get,
            "post" => PathItemType::Post,
            _ => unreachable!("phase-one OpenAPI method is fixed"),
        };
        let operation = OperationBuilder::new()
            .summary(Some(summary))
            .response("200", ResponseBuilder::new().description("Success").build())
            .build();
        document
            .paths
            .paths
            .insert(path.to_string(), PathItem::new(method, operation));
    }
    document
}

pub fn openapi_json() -> Result<String> {
    serde_json::to_string_pretty(&openapi()).context("serialize OpenAPI contract")
}

pub(crate) fn retention_rejection(event: &EventEnvelope) -> Option<String> {
    retention_rejection_at(event, Utc::now())
}
