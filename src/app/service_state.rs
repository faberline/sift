//! The state every handler shares: the durable journal, the drain bit read by
//! `/readyz`, the Raft host when replicated, the projections, admission, local
//! capacity, query jobs and the ingest batch coordinator.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::ingest::application::admission_controller::AdmissionController;
use crate::ingest::domain::ingest_limits::IngestLimits;
use crate::ingest::infrastructure::ingest_batch_coordinator::IngestBatchCoordinator;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::journal::infrastructure::raft::sift_membership_policy::SiftMembershipPolicy;
use crate::node::domain::storage_role::StorageRole;
use crate::node::infrastructure::local_capacity::LocalCapacity;
use crate::projection::application::projection_runtime::ProjectionRuntime;
use crate::query::infrastructure::file_query_job_store::QueryJobStore;

/// Shared HTTP state: journal access plus the drain bit read by `/readyz`.
#[derive(Clone)]
pub struct ServiceState {
    pub(crate) journal: Arc<DurableJournal>,
    pub(crate) draining: Arc<AtomicBool>,
    pub(crate) raft: Option<Arc<raft_runtime::RaftHost>>,
    pub(crate) peer_transport: Option<raft_runtime::PeerTransport>,
    pub(crate) peer_port: Option<u16>,
    pub(crate) state_machine:
        Arc<crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine>,
    pub(crate) local_command: Arc<tokio::sync::Mutex<()>>,
    pub(crate) projections: Arc<ProjectionRuntime>,
    pub(crate) admission: Arc<AdmissionController>,
    pub(crate) local_capacity: Arc<LocalCapacity>,
    pub(crate) query_jobs: Arc<QueryJobStore>,
    pub(crate) batch_coordinator: Arc<std::sync::Mutex<IngestBatchCoordinator>>,
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
            projections: Arc::new(ProjectionRuntime::open(data_dir, journal.clone())?),
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

    pub fn projections(&self) -> &ProjectionRuntime {
        &self.projections
    }
}
