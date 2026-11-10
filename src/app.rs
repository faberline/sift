//! The assembly root: the service state every context's handlers share, the
//! readiness and metrics hooks service_http reads, the HTTP data plane with
//! its API error and OpenAPI document, and the event constructors that read
//! the system clock. The app wires the contexts together and owns no domain
//! rules of its own.

mod access_wiring;
pub(crate) mod archive_wiring;
pub(crate) mod collector_metrics;
pub(crate) mod collector_wiring;
pub(crate) mod grpc_authorization;
mod instance_wiring;
pub(crate) mod interfaces;
pub(crate) mod otlp_wiring;
pub(crate) mod service_state;
pub(crate) mod system_clock;

mod journal_command_wiring;

mod journal_storage_wiring;

mod projection_wiring;

mod query_error_wiring;

#[cfg(test)]
mod collector_delivery_tests;
mod control_jobs;
mod control_observations;
pub(crate) mod control_wiring;
#[cfg(test)]
mod metric_transport_tests;
#[cfg(test)]
mod otlp_delivery_tests;
#[cfg(test)]
mod production_tests;
pub(crate) mod query_wiring;
pub(crate) mod recovery_wiring;

pub(crate) mod checkpoint_wiring;

mod peer_server_wiring;

/// Shared service instances. Only assembly can access the concrete backends.
#[derive(Clone)]
pub struct ServiceState {
    query_failures: std::sync::Arc<std::sync::atomic::AtomicU64>,
    storage_role: crate::node::domain::storage_role::StorageRole,
    recovery_guard: std::sync::Arc<crate::operations::domain::recovery_guard::RecoveryGuard>,
    archive_serial: std::sync::Arc<tokio::sync::Mutex<()>>,
    control_serial: std::sync::Arc<tokio::sync::Mutex<()>>,
    control: Option<std::sync::Arc<crate::control::application::service::ControlService>>,
    journal: std::sync::Arc<crate::journal::application::journal_service::DurableJournal>,
    draining: std::sync::Arc<std::sync::atomic::AtomicBool>,
    raft: Option<std::sync::Arc<raft_runtime::RaftHost>>,
    peer_transport: Option<raft_runtime::PeerTransport>,
    peer_port: Option<u16>,
    checkpoints:
        std::sync::Arc<crate::journal::application::checkpoint_service::JournalCheckpointService>,
    checkpoint_raft: Option<
        std::sync::Arc<crate::journal::application::raft_checkpoint_service::RaftCheckpointService>,
    >,
    projections:
        std::sync::Arc<crate::projection::application::projection_runtime::ProjectionRuntime>,
    admission:
        std::sync::Arc<crate::ingest::application::admission_controller::AdmissionController>,
    local_capacity: std::sync::Arc<crate::node::infrastructure::local_capacity::LocalCapacity>,
    query_jobs: std::sync::Arc<crate::query::application::job_service::QueryJobService>,
    ingest: std::sync::Arc<crate::ingest::application::ingest_service::IngestService>,
    archive_reads: std::sync::Arc<crate::archive::application::read_service::ArchiveReadService>,
    archive_lifecycle:
        std::sync::Arc<crate::archive::application::lifecycle_service::ArchiveLifecycleService>,
}
