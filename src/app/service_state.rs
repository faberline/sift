//! The state every handler shares: the durable journal, the drain bit read by
//! `/readyz`, the Raft host when replicated, the projections, admission, local
//! capacity, query jobs and the ingest batch coordinator.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::ingest::application::admission_controller::AdmissionController;
use crate::ingest::domain::batch_submission::BatchSubmission;
use crate::ingest::domain::ingest_limits::IngestLimits;
use crate::ingest::infrastructure::ingest_batch_coordinator::{
    IngestBatchBackend, IngestBatchCoordinator,
};
use crate::journal::application::journal_service::DurableJournal;
use crate::journal::infrastructure::raft::sift_membership_policy::SiftMembershipPolicy;
use crate::node::domain::storage_role::StorageRole;
use crate::node::infrastructure::local_capacity::LocalCapacity;
use crate::projection::application::projection_runtime::ProjectionRuntime;
use crate::query::application::job_service::QueryJobService;

pub use super::ServiceState;

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
        let (checkpoints, checkpoint_raft) =
            super::checkpoint_wiring::checkpoint_services(state_machine.clone(), raft.as_ref());
        let draining = Arc::new(AtomicBool::new(false));
        let local_command = Arc::new(tokio::sync::Mutex::new(()));
        let context = super::journal_command_wiring::commit_context(
            journal.clone(),
            raft.clone(),
            state_machine.clone(),
            local_command.clone(),
            local_capacity.clone(),
        );
        let journal_batches = Arc::new(
            crate::journal::application::batch_limits::JournalBatchSizing::new(
                context.commands.clone(),
            ),
        );
        // Keep the original initialization order for the three I/O services.
        let projections = Arc::new(ProjectionRuntime::open(data_dir, journal.clone())?);
        let admission = Arc::new(AdmissionController::new(limits)?);
        let query_jobs = Arc::new(QueryJobService::open(data_dir.join("query-jobs"))?);
        let batch_submission: Arc<dyn BatchSubmission<Error = anyhow::Error>> =
            Arc::new(IngestBatchBackend {
                coordinator: std::sync::Mutex::new(IngestBatchCoordinator::default()),
                draining: draining.clone(),
                context: context.clone(),
                sizing: journal_batches.clone(),
            });
        let ingest = Arc::new(
            crate::ingest::application::ingest_service::IngestService::new(
                journal.clone(),
                draining.clone(),
                context,
                journal_batches,
                batch_submission,
                Arc::new(
                    crate::ingest::infrastructure::capacity_adapter::NativeIngestCapacity {
                        capacity: local_capacity.clone(),
                    },
                ),
            ),
        );
        let archive_services = Arc::new(super::archive_wiring::archive_services());
        Ok(Self {
            projections,
            journal,
            draining: draining.clone(),
            raft,
            peer_transport,
            peer_port,
            checkpoints,
            checkpoint_raft,
            admission,
            local_capacity,
            query_jobs,
            ingest,
            archive_reads: Arc::new(
                crate::archive::application::read_service::ArchiveReadService::new(
                    archive_services.clone(),
                ),
            ),
            archive_lifecycle: Arc::new(
                crate::archive::application::lifecycle_service::ArchiveLifecycleService::new(
                    archive_services,
                ),
            ),
        })
    }

    pub fn journal(&self) -> &DurableJournal {
        &self.journal
    }

    pub fn projections(&self) -> &ProjectionRuntime {
        &self.projections
    }

    pub(crate) fn journal_service(&self) -> Arc<DurableJournal> {
        self.journal.clone()
    }

    pub(crate) fn projection_service(&self) -> Arc<ProjectionRuntime> {
        self.projections.clone()
    }

    pub(crate) fn admission(&self) -> &AdmissionController {
        &self.admission
    }

    pub(crate) fn query_jobs(&self) -> &Arc<QueryJobService> {
        &self.query_jobs
    }

    pub(crate) fn archive_reads(
        &self,
    ) -> &crate::archive::application::read_service::ArchiveReadService {
        &self.archive_reads
    }

    pub(crate) fn ingest_service(
        &self,
    ) -> &crate::ingest::application::ingest_service::IngestService {
        &self.ingest
    }

    pub async fn append_batch(
        &self,
        events: Vec<crate::shared_kernel::event::EventEnvelope>,
    ) -> Result<Vec<crate::shared_kernel::append_result::AppendResult>> {
        self.ingest_service().append_batch(events).await
    }

    pub async fn append_events(
        &self,
        events: Vec<crate::shared_kernel::event::EventEnvelope>,
    ) -> Result<Vec<crate::shared_kernel::append_result::AppendResult>> {
        self.ingest_service().append_events(events).await
    }

    pub fn start_drain(&self) {
        self.ingest_service().start_drain();
    }

    pub async fn finish_drain(&self) -> Result<()> {
        self.ingest_service().finish_drain().await
    }

    pub fn is_draining(&self) -> bool {
        self.ingest_service().is_draining()
    }

    pub(crate) fn ensure_local_capacity(
        &self,
        incoming_bytes: usize,
    ) -> Result<(), crate::ingest::domain::admission_error::AdmissionError> {
        self.ingest_service().ensure_local_capacity(incoming_bytes)
    }
}

#[cfg(test)]
mod tests;
