//! Sift's service core for logs, metrics, and traces. The canonical per-signal
//! WAL is fsynced before acknowledgement. Rebuildable indexes are never a
//! second source of truth.

pub mod api;
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
pub mod operator;
pub mod projection;
pub mod prometheus;
pub mod proxy;
mod query;
mod shared_kernel;
pub mod storage;

pub use crate::ingest::domain::governance_policy::{GovernancePolicy, GovernancePolicySet};
pub use crate::journal::domain::append_result::AppendResult;
pub use crate::journal::domain::event_query::EventQuery;
pub use crate::journal::infrastructure::durable_journal::DurableJournal;
pub use crate::journal::infrastructure::journal_projection_read_session::JournalProjectionReadSession;
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

use anyhow::{bail, Context, Result};
use axum::{
    body::Body,
    extract::{Extension, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use metrics_prometheus::Sample;
use serde::{Deserialize, Serialize};
use service_auth::{Role, RoleMapPrincipal};
use service_http::{DetailedErrorEnvelope as ErrorEnvelope, ProjectionMetadata};
use sha2::{Digest, Sha256};
use utoipa::{OpenApi, ToSchema};

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
use crate::query::application::archive_query_status::ArchiveQueryStatus;
use crate::query::infrastructure::file_query_job_store::QueryJobStore;
use crate::query::interfaces::http::correlate_v1::correlate_v1;
use crate::query::interfaces::http::get_trace::get_trace;
use crate::query::interfaces::http::list_services_v1::list_services_v1;
use crate::query::interfaces::http::prometheus_query::{
    prometheus_instant_query, prometheus_range_query,
};
use crate::query::interfaces::http::query_request_v1::QueryRequestV1;
use crate::query::interfaces::http::query_v1::{get_query_job_v1, query_v1, tail_logs_v1};
use crate::query::interfaces::mcp::mcp_transport::http_router;
use crate::shared_kernel::retention_boundary::retention_rejection_at;

const ALL_VOTER_CHECKPOINT_ATTEMPT: std::time::Duration = std::time::Duration::from_secs(30);
const ARCHIVE_GC_BATCH_OBJECTS: usize = 128;

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
    local_capacity: Arc<storage::LocalCapacity>,
    query_jobs: Arc<QueryJobStore>,
    batch_coordinator: Arc<std::sync::Mutex<IngestBatchCoordinator>>,
}

impl ServiceState {
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_role(data_dir, storage::StorageRole::All)
    }

    pub fn open_with_role(data_dir: impl AsRef<Path>, role: storage::StorageRole) -> Result<Self> {
        Self::open_with_ingest_limits_and_role(data_dir, IngestLimits::from_env()?, role)
    }

    pub fn open_with_ingest_limits(
        data_dir: impl AsRef<Path>,
        limits: IngestLimits,
    ) -> Result<Self> {
        Self::open_with_ingest_limits_and_role(data_dir, limits, storage::StorageRole::All)
    }

    pub fn open_with_ingest_limits_and_role(
        data_dir: impl AsRef<Path>,
        limits: IngestLimits,
        role: storage::StorageRole,
    ) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        let journal = Arc::new(DurableJournal::open_with_role(data_dir, role)?);
        let local_capacity = Arc::new(storage::LocalCapacity::open(
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

    /// Start the store lifecycle worker. A replicated follower stays idle.
    /// The worker records the remote manifest before it permits WAL compaction.
    pub fn start_archive_worker(
        &self,
        destination: impl Into<String>,
        interval: std::time::Duration,
    ) -> ArchiveWorker {
        self.start_lifecycle_worker(Some(destination.into()), interval)
    }

    /// Start the same leader-only lifecycle worker for an installation that
    /// has no remote archive. It commits the local immutable segment set before
    /// it compacts WAL bytes.
    pub fn start_local_archive_worker(&self, interval: std::time::Duration) -> ArchiveWorker {
        self.start_lifecycle_worker(None, interval)
    }

    fn start_lifecycle_worker(
        &self,
        destination: Option<String>,
        interval: std::time::Duration,
    ) -> ArchiveWorker {
        let journal = self.journal.clone();
        let raft = self.raft.clone();
        let state_machine = self.state_machine.clone();
        let local_capacity = self.local_capacity.clone();
        let (shutdown, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(async move {
            loop {
                let leader = match &raft {
                    Some(raft) => raft.is_leader().await,
                    None => true,
                };
                if leader {
                    let remote_archive = destination.is_some();
                    let replicated = raft.is_some();
                    let retention_capable = if remote_archive && replicated {
                        match raft
                            .as_ref()
                            .expect("replicated lifecycle has a Raft host")
                            .require_snapshot_capability_on_all_voters()
                            .await
                        {
                            Ok(()) => true,
                            Err(error) => {
                                tracing::warn!(
                                    %error,
                                    "Sift retention waits for every voter snapshot capability"
                                );
                                false
                            }
                        }
                    } else {
                        true
                    };
                    let retention_fence = prepare_retention_fence(
                        &journal,
                        &state_machine,
                        raft.as_ref(),
                        remote_archive,
                        retention_capable,
                    )
                    .await;
                    let archive = match retention_fence {
                        Ok(retention_fence) => {
                            let attempt_journal = journal.clone();
                            let attempt_state_machine = state_machine.clone();
                            let attempt_destination = destination.clone();
                            match tokio::task::spawn_blocking(move || {
                                run_lifecycle_attempt(
                                    &attempt_journal,
                                    &attempt_state_machine,
                                    attempt_destination.as_deref(),
                                    retention_fence,
                                )
                            })
                            .await
                            {
                                Ok(result) => result,
                                Err(error) => Err(anyhow::anyhow!(
                                    "Sift archive worker task panicked: {error}"
                                )),
                            }
                        }
                        Err(error) => Err(error),
                    };
                    match archive {
                        Ok(mut outcome) => {
                            let mut changed = outcome.commit.is_some();
                            let mut replicated_checkpoint_installed = false;
                            let still_leader = match &raft {
                                Some(raft) => raft.is_leader().await,
                                None => true,
                            };
                            if !still_leader {
                                tracing::warn!(
                                    "Sift lifecycle lost leadership after archive work; checkpoint and GC are deferred"
                                );
                            }
                            if let Some(raft) = &raft {
                                let mut checkpoint_allowed =
                                    still_leader && outcome.captured_applied_index > 0;
                                if outcome.retention_scan_pending {
                                    checkpoint_allowed = false;
                                    tracing::debug!(
                                        "Sift keeps the retention fence and Raft log until the bounded scan completes"
                                    );
                                }
                                let mut quorum_only_checkpoint = false;
                                if checkpoint_allowed
                                    && remote_archive
                                    && outcome.pending_archive_gc
                                {
                                    if !retention_capable {
                                        checkpoint_allowed = false;
                                        if let (
                                            Some(retention_generation),
                                            Some(manifest_uri),
                                            Some(manifest_sha256),
                                        ) = (
                                            outcome.retention_generation,
                                            outcome.manifest_uri.clone(),
                                            outcome.manifest_sha256.clone(),
                                        ) {
                                            match (crate::journal::domain::sift_command::SiftCommandV1::ArchiveCheckpointBarrier {
                                                    retention_generation,
                                                    manifest_uri,
                                                    manifest_sha256,
                                                })
                                                .encoded()
                                            {
                                                Ok(command) => match raft.propose(command).await {
                                                    Ok(index) => {
                                                        outcome.captured_applied_index = index;
                                                        quorum_only_checkpoint = true;
                                                    }
                                                    Err(error) => tracing::warn!(
                                                        %error,
                                                        "Sift no-GC quorum checkpoint barrier proposal failed"
                                                    ),
                                                },
                                                Err(error) => tracing::warn!(
                                                    %error,
                                                    "Sift no-GC quorum checkpoint barrier encoding failed"
                                                ),
                                            }
                                        } else {
                                            tracing::warn!(
                                                "Sift archive GC is pending without a checkpoint identity"
                                            );
                                        }
                                    } else if let (
                                        Some(retention_generation),
                                        Some(manifest_uri),
                                        Some(manifest_sha256),
                                    ) = (
                                        outcome.retention_generation,
                                        outcome.manifest_uri.clone(),
                                        outcome.manifest_sha256.clone(),
                                    ) {
                                        match (crate::journal::domain::sift_command::SiftCommandV1::ArchiveCheckpointBarrier {
                                                retention_generation,
                                                manifest_uri,
                                                manifest_sha256,
                                            })
                                            .encoded()
                                        {
                                            Ok(command) => match raft.propose(command).await {
                                                Ok(index) => {
                                                    outcome.captured_applied_index = index;
                                                }
                                                Err(error) => {
                                                    checkpoint_allowed = false;
                                                    tracing::warn!(
                                                        %error,
                                                        "Sift retention barrier proposal failed"
                                                    );
                                                }
                                            },
                                            Err(error) => {
                                                checkpoint_allowed = false;
                                                tracing::warn!(
                                                    %error,
                                                    "Sift retention barrier encoding failed"
                                                );
                                            }
                                        }
                                    } else {
                                        checkpoint_allowed = false;
                                        tracing::warn!(
                                            "Sift archive GC is pending without a retention generation"
                                        );
                                    }
                                }
                                if checkpoint_allowed {
                                    let checkpoint_journal = journal.clone();
                                    let checkpoint_state_machine = state_machine.clone();
                                    let checkpoint_destination = destination.clone();
                                    let prepared = tokio::task::spawn_blocking(move || {
                                        prepare_lifecycle_checkpoint(
                                            &checkpoint_journal,
                                            &checkpoint_state_machine,
                                            checkpoint_destination.as_deref(),
                                            true,
                                        )
                                    })
                                    .await;
                                    match prepared {
                                        Ok(Ok(up_to)) => {
                                            let all_voter_checkpoint = tokio::time::timeout(
                                                ALL_VOTER_CHECKPOINT_ATTEMPT,
                                                raft.snapshot_and_compact_through_outcome(up_to),
                                            )
                                            .await
                                            .map_err(|_| {
                                                anyhow::anyhow!(
                                                    "Sift all-voter checkpoint attempt exceeded {} seconds",
                                                    ALL_VOTER_CHECKPOINT_ATTEMPT.as_secs()
                                                )
                                            })
                                            .and_then(|result| result);
                                            match all_voter_checkpoint {
                                                Ok(compaction) if compaction.installed => {
                                                    changed = true;
                                                    replicated_checkpoint_installed = true;
                                                    if let Some(retention_generation) =
                                                        outcome.retention_generation
                                                    {
                                                        if let Err(error) =
                                                            clear_completed_retention_fence(
                                                                raft,
                                                                &state_machine,
                                                                retention_generation,
                                                            )
                                                            .await
                                                        {
                                                            tracing::warn!(
                                                                %error,
                                                                "Sift retention checkpoint installed but its quorum fence-clear command failed"
                                                            );
                                                        }
                                                    }
                                                    tracing::info!(
                                                        compacted_raft_index =
                                                            compaction.snapshot_index,
                                                        "Sift archived Raft prefix compacted"
                                                    );
                                                }
                                                Ok(_) => {}
                                                Err(error) => {
                                                    tracing::warn!(
                                                        %error,
                                                        up_to,
                                                        "Sift all-voter checkpoint failed; retain the Raft prefix for voter catch-up"
                                                    );
                                                    if remote_archive {
                                                        match compact_remote_quorum_without_gc(
                                                            &journal,
                                                            &state_machine,
                                                            &destination,
                                                            raft,
                                                        )
                                                        .await
                                                        {
                                                            Ok(compaction)
                                                                if compaction.installed =>
                                                            {
                                                                changed = true;
                                                                if let Some(retention_generation) =
                                                                    outcome.retention_generation
                                                                {
                                                                    if let Err(error) =
                                                                        clear_completed_retention_fence(
                                                                            raft,
                                                                            &state_machine,
                                                                            retention_generation,
                                                                        )
                                                                        .await
                                                                    {
                                                                        tracing::warn!(
                                                                            %error,
                                                                            "Sift quorum checkpoint installed but its fence-clear command failed"
                                                                        );
                                                                    }
                                                                }
                                                                tracing::info!(
                                                                    compacted_raft_index =
                                                                        compaction.snapshot_index,
                                                                    "Sift compacted a GCS-backed Raft prefix on quorum; archive GC waits for every voter"
                                                                );
                                                            }
                                                            Ok(_) => {}
                                                            Err(quorum_error) => tracing::warn!(
                                                                %quorum_error,
                                                                up_to,
                                                                "Sift quorum archive checkpoint also failed"
                                                            ),
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        Ok(Err(error)) => {
                                            tracing::warn!(
                                                %error,
                                                "Sift checkpoint preparation failed; try resident all-voter checkpoint"
                                            );
                                            if let Err(fallback_error) =
                                                compact_resident_all_voters(&state_machine, raft)
                                                    .await
                                            {
                                                tracing::warn!(
                                                    %fallback_error,
                                                    "Sift resident all-voter checkpoint failed"
                                                );
                                            } else {
                                                changed = true;
                                            }
                                        }
                                        Err(error) => tracing::warn!(
                                            %error,
                                            "Sift checkpoint preparation task panicked"
                                        ),
                                    }
                                }
                                if quorum_only_checkpoint {
                                    match compact_remote_quorum_without_gc(
                                        &journal,
                                        &state_machine,
                                        &destination,
                                        raft,
                                    )
                                    .await
                                    {
                                        Ok(compaction) if compaction.installed => {
                                            changed = true;
                                            if let Some(retention_generation) =
                                                outcome.retention_generation
                                            {
                                                if let Err(error) = clear_completed_retention_fence(
                                                    raft,
                                                    &state_machine,
                                                    retention_generation,
                                                )
                                                .await
                                                {
                                                    tracing::warn!(
                                                        %error,
                                                        "Sift no-GC quorum checkpoint installed but its fence-clear command failed"
                                                    );
                                                }
                                            }
                                            tracing::info!(
                                                    compacted_raft_index =
                                                        compaction.snapshot_index,
                                                    "Sift installed a no-GC archive checkpoint on quorum; every prior archive object is retained"
                                                );
                                        }
                                        Ok(_) => {}
                                        Err(error) => tracing::warn!(
                                            %error,
                                            "Sift no-GC quorum archive checkpoint failed"
                                        ),
                                    }
                                }
                            }
                            let archive_gc_is_safe = still_leader
                                && remote_archive
                                && !outcome.retention_scan_pending
                                && (!replicated || replicated_checkpoint_installed);
                            if archive_gc_is_safe {
                                let gc_journal = journal.clone();
                                match tokio::task::spawn_blocking(move || {
                                    storage::archive::finish_local_blob_gc(&gc_journal)?;
                                    storage::archive::finalize_archive_gc_batch_after_checkpoint(
                                        gc_journal.storage().root(),
                                        ARCHIVE_GC_BATCH_OBJECTS,
                                    )
                                })
                                .await
                                {
                                    Ok(Ok((deleted, complete))) if deleted > 0 => tracing::info!(
                                        deleted_archive_objects = deleted,
                                        archive_gc_complete = complete,
                                        "Sift obsolete archive objects deleted after checkpoint"
                                    ),
                                    Ok(Ok((_, false))) => tracing::debug!(
                                        archive_gc_batch_objects = ARCHIVE_GC_BATCH_OBJECTS,
                                        "Sift archive GC saved its cursor for the next lifecycle pass"
                                    ),
                                    Ok(Ok((_, true))) => {}
                                    Ok(Err(error)) => tracing::warn!(
                                        %error,
                                        "Sift archive checkpoint committed but obsolete object cleanup failed"
                                    ),
                                    Err(error) => tracing::warn!(
                                        %error,
                                        "Sift archive cleanup task panicked after checkpoint"
                                    ),
                                }
                            }
                            if changed {
                                if let Err(error) = local_capacity.reconcile() {
                                    tracing::warn!(
                                        %error,
                                        "Sift lifecycle committed but capacity reconciliation failed"
                                    );
                                }
                            }
                            if let Some(receipt) = outcome.commit {
                                tracing::info!(
                                    manifest_uri =
                                        receipt.manifest_uri.as_deref().unwrap_or("local"),
                                    event_count = receipt.event_count,
                                    segment_count = receipt.segment_count,
                                    "Sift lifecycle manifest committed"
                                );
                            }
                        }
                        Err(error) => {
                            tracing::warn!(
                                %error,
                                "Sift archive attempt failed; WAL remains uncompacted"
                            );
                            if let Some(raft) = &raft {
                                if raft.is_leader().await {
                                    if let Err(fallback_error) =
                                        compact_resident_all_voters(&state_machine, raft).await
                                    {
                                        tracing::warn!(
                                            %fallback_error,
                                            "Sift resident all-voter checkpoint failed after archive outage"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(interval.max(std::time::Duration::from_millis(1))) => {}
                }
            }
        });
        ArchiveWorker {
            shutdown: Some(shutdown),
            task,
        }
    }
}

pub struct ArchiveWorker {
    shutdown: Option<tokio::sync::watch::Sender<bool>>,
    task: tokio::task::JoinHandle<()>,
}

struct LifecycleCommit {
    manifest_uri: Option<String>,
    event_count: u64,
    segment_count: usize,
}

#[derive(Default)]
struct LifecycleOutcome {
    commit: Option<LifecycleCommit>,
    captured_applied_index: u64,
    pending_archive_gc: bool,
    retention_generation: Option<u64>,
    manifest_uri: Option<String>,
    manifest_sha256: Option<String>,
    retention_scan_pending: bool,
}

async fn prepare_retention_fence(
    journal: &Arc<DurableJournal>,
    state_machine: &Arc<crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine>,
    raft: Option<&Arc<raft_runtime::RaftHost>>,
    remote_archive: bool,
    retention_capable: bool,
) -> Result<Option<crate::journal::domain::retention_fence::RetentionFenceV1>> {
    if let Some((fence, applied_index)) = state_machine.pending_retention_fence() {
        if let Some(raft) = raft {
            raft.require_applied_index_on_all_voters(applied_index)
                .await
                .context("wait for every Sift voter to apply the retention fence")?;
        }
        if let Some(status) = storage::archive::committed_status(journal.storage().root())? {
            if status.retention_scan_pending
                && status.retention_generation >= fence.target_generation
            {
                let next = crate::journal::domain::retention_fence::RetentionFenceV1 {
                    source_manifest_uri: status.manifest_uri,
                    source_manifest_sha256: status.manifest_sha256,
                    target_generation: status.retention_generation.saturating_add(1),
                    evaluate_at: fence.evaluate_at,
                };
                if let Some(raft) = raft {
                    let command =
                        crate::journal::domain::sift_command::SiftCommandV1::RetentionFence {
                            fence: next.clone(),
                        }
                        .encoded()?;
                    let fence_index = raft
                        .propose(command)
                        .await
                        .context("advance the bounded Sift retention fence")?;
                    raft.require_applied_index_on_all_voters(fence_index)
                        .await
                        .context(
                            "wait for every Sift voter to apply the advanced retention fence",
                        )?;
                    return state_machine
                        .pending_retention_fence()
                        .context("advanced Sift retention fence was not applied locally")
                        .map(|(fence, _)| Some(fence));
                }
                return Ok(Some(next));
            }
        }
        return Ok(Some(fence));
    }
    if !remote_archive || !retention_capable {
        return Ok(None);
    }
    let evaluate_at = Utc::now();
    if !storage::archive::retention_due_at(journal.storage().root(), evaluate_at)? {
        return Ok(None);
    }
    let status = storage::archive::committed_status(journal.storage().root())?
        .context("Sift retention requires a committed archive")?;
    let fence = crate::journal::domain::retention_fence::RetentionFenceV1 {
        source_manifest_uri: status.manifest_uri,
        source_manifest_sha256: status.manifest_sha256,
        target_generation: status.retention_generation.saturating_add(1),
        evaluate_at: evaluate_at.to_rfc3339(),
    };
    if let Some(raft) = raft {
        let command = crate::journal::domain::sift_command::SiftCommandV1::RetentionFence {
            fence: fence.clone(),
        }
        .encoded()?;
        let fence_index = raft
            .propose(command)
            .await
            .context("commit Sift retention fence")?;
        raft.require_applied_index_on_all_voters(fence_index)
            .await
            .context("wait for every Sift voter to apply the retention fence")?;
        return state_machine
            .pending_retention_fence()
            .context("committed Sift retention fence was not applied locally")
            .map(|(fence, _)| Some(fence));
    }
    Ok(Some(fence))
}

fn run_lifecycle_attempt(
    journal: &DurableJournal,
    state_machine: &crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine,
    destination: Option<&str>,
    retention_fence: Option<crate::journal::domain::retention_fence::RetentionFenceV1>,
) -> Result<LifecycleOutcome> {
    storage::archive::reconcile_live_committed_retention(journal)?;
    storage::archive::reconcile_committed_wal(journal)?;
    storage::archive::resume_local_blob_gc_batch(journal, 128, 1_280_000)?;
    let (applied_index, raw_cursor, segments) = state_machine.capture_archive_prefix()?;
    match destination {
        Some(destination) => {
            let committed_cursor = storage::archive::committed_status(journal.storage().root())?
                .map(|status| status.snapshot_index)
                .unwrap_or_default();
            let mut commit = if retention_fence.is_none() && raw_cursor > committed_cursor {
                let receipt = storage::archive::archive_journal_gcs_captured(
                    journal,
                    destination,
                    raw_cursor,
                    segments,
                )?;
                Some(LifecycleCommit {
                    manifest_uri: Some(receipt.manifest_uri),
                    event_count: receipt.manifest.event_count,
                    segment_count: receipt.manifest.segment_count as usize,
                })
            } else {
                None
            };
            if storage::archive::committed_status(journal.storage().root())?.is_some() {
                storage::archive::evict_committed_cold_segments_at(journal, Utc::now())?;
                if let Some(fence) = retention_fence {
                    let status = storage::archive::committed_status(journal.storage().root())?
                        .context("Sift retention fence requires a committed archive")?;
                    if status.retention_generation < fence.target_generation {
                        if status.manifest_uri != fence.source_manifest_uri
                            || status.manifest_sha256 != fence.source_manifest_sha256
                            || status.retention_generation.saturating_add(1)
                                != fence.target_generation
                        {
                            bail!("Sift retention fence source no longer matches local archive");
                        }
                        let evaluate_at = DateTime::parse_from_rfc3339(&fence.evaluate_at)
                            .context("Sift retention fence time must be RFC3339")?
                            .with_timezone(&Utc);
                        if let Some(expired) =
                            state_machine.expire_current_archive_at(evaluate_at)?
                        {
                            commit = Some(LifecycleCommit {
                                manifest_uri: Some(expired.manifest_uri),
                                event_count: expired.retained_events,
                                segment_count: expired.retained_segments,
                            });
                        }
                    } else if status.retention_generation > fence.target_generation {
                        bail!("Sift retention fence target is behind local retention");
                    }
                }
            }
            let status = storage::archive::committed_status(journal.storage().root())?;
            Ok(LifecycleOutcome {
                commit,
                captured_applied_index: applied_index,
                pending_archive_gc: storage::archive::archive_gc_pending(journal.storage().root()),
                retention_generation: status.as_ref().map(|status| status.retention_generation),
                manifest_uri: status.as_ref().map(|status| status.manifest_uri.clone()),
                manifest_sha256: status.as_ref().map(|status| status.manifest_sha256.clone()),
                retention_scan_pending: status
                    .as_ref()
                    .is_some_and(|status| status.retention_scan_pending),
            })
        }
        None => {
            let committed_cursor =
                storage::archive::local_committed_watermarks(journal.storage().root())?
                    .max_cursor();
            let commit = if raw_cursor > committed_cursor {
                let receipt = storage::archive::archive_journal_local_captured(
                    journal, raw_cursor, segments,
                )?;
                Some(LifecycleCommit {
                    manifest_uri: None,
                    event_count: receipt.event_count,
                    segment_count: receipt.segment_count,
                })
            } else {
                None
            };
            Ok(LifecycleOutcome {
                commit,
                captured_applied_index: applied_index,
                ..LifecycleOutcome::default()
            })
        }
    }
}

fn prepare_lifecycle_checkpoint(
    journal: &DurableJournal,
    state_machine: &crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine,
    destination: Option<&str>,
    archive_gc_authorized: bool,
) -> Result<u64> {
    let (applied_index, raw_cursor, segments) = state_machine.capture_archive_prefix()?;
    if applied_index == 0 {
        return Ok(0);
    }
    match destination {
        Some(destination) => {
            let committed_cursor = storage::archive::committed_status(journal.storage().root())?
                .map(|status| status.snapshot_index)
                .unwrap_or_default();
            if raw_cursor > committed_cursor {
                storage::archive::archive_journal_gcs_captured(
                    journal,
                    destination,
                    raw_cursor,
                    segments,
                )?;
            }
            if archive_gc_authorized {
                state_machine.prepare_archive_checkpoint(applied_index, raw_cursor)?;
            } else {
                state_machine.prepare_archive_checkpoint_without_gc(applied_index, raw_cursor)?;
            }
        }
        None => {
            let committed_cursor =
                storage::archive::local_committed_watermarks(journal.storage().root())?
                    .max_cursor();
            if raw_cursor > committed_cursor {
                storage::archive::archive_journal_local_captured(journal, raw_cursor, segments)?;
            }
            state_machine.prepare_local_checkpoint(applied_index, raw_cursor)?;
        }
    }
    Ok(applied_index)
}

async fn compact_remote_quorum_without_gc(
    journal: &Arc<DurableJournal>,
    state_machine: &Arc<crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine>,
    destination: &Option<String>,
    raft: &Arc<raft_runtime::RaftHost>,
) -> Result<raft_runtime::SnapshotCompactionOutcome> {
    let checkpoint_journal = journal.clone();
    let checkpoint_state_machine = state_machine.clone();
    let checkpoint_destination = destination.clone();
    let up_to = tokio::task::spawn_blocking(move || {
        prepare_lifecycle_checkpoint(
            &checkpoint_journal,
            &checkpoint_state_machine,
            checkpoint_destination.as_deref(),
            false,
        )
    })
    .await
    .context("Sift no-GC quorum checkpoint preparation task panicked")??;
    raft.snapshot_and_compact_through_quorum_outcome(up_to)
        .await
}

async fn clear_completed_retention_fence(
    raft: &Arc<raft_runtime::RaftHost>,
    state_machine: &Arc<crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine>,
    retention_generation: u64,
) -> Result<()> {
    let Some((fence, _)) = state_machine.pending_retention_fence() else {
        return Ok(());
    };
    if fence.target_generation > retention_generation {
        return Ok(());
    }
    raft.propose(
        crate::journal::domain::sift_command::SiftCommandV1::clear_retention_fence(
            retention_generation,
        )
        .encoded()?,
    )
    .await
    .context("commit Sift retention fence clear to quorum")?;
    Ok(())
}

async fn compact_resident_all_voters(
    state_machine: &Arc<crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine>,
    raft: &Arc<raft_runtime::RaftHost>,
) -> Result<raft_runtime::SnapshotCompactionOutcome> {
    let checkpoint_state_machine = state_machine.clone();
    let up_to = tokio::task::spawn_blocking(move || {
        let (applied_index, raw_cursor, _) = checkpoint_state_machine.capture_archive_prefix()?;
        if applied_index > 0 {
            checkpoint_state_machine.prepare_resident_checkpoint(applied_index, raw_cursor)?;
        }
        anyhow::Ok(applied_index)
    })
    .await
    .context("Sift resident checkpoint preparation task panicked")??;
    raft.snapshot_and_compact_through_outcome(up_to).await
}

impl ArchiveWorker {
    pub async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(true);
        }
        let _ = self.task.await;
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
pub fn protected_router(state: Arc<ServiceState>, verifier: Arc<auth::SiftVerifier>) -> Router {
    router(state).layer(axum::middleware::from_fn_with_state(
        verifier,
        auth::auth_middleware,
    ))
}

/// Build the protected HTTP data plane plus the official MCP Streamable HTTP
/// endpoint. MCP tools forward the caller's credential to these same routes.
pub fn protected_router_with_mcp(
    state: Arc<ServiceState>,
    verifier: Arc<auth::SiftVerifier>,
    internal_endpoint: &str,
) -> Result<Router> {
    Ok(router(state).merge(http_router(internal_endpoint)?).layer(
        axum::middleware::from_fn_with_state(verifier, auth::auth_middleware),
    ))
}

fn replay_cold_query(
    state: &ServiceState,
    request: &QueryRequestV1,
    signal: SignalKind,
    projection: &dyn projection::Projection,
) -> ArchiveQueryStatus {
    let replay = storage::archive::replay_committed_events(
        state.journal().storage().root(),
        signal,
        &request.project,
        request.environment.as_deref(),
        request.time_range.start.as_deref(),
        request.time_range.end.as_deref(),
        |event| projection.apply_idempotent(&event),
    );
    let replay = match replay {
        Ok(Some(replay)) => replay,
        Ok(None) => {
            return ArchiveQueryStatus::Unavailable(
                "archive manifest is not committed for the requested cold time range".to_string(),
            )
        }
        Err(error) => {
            return ArchiveQueryStatus::Unavailable(format!(
                "archive is unavailable for the requested cold time range: {error}"
            ))
        }
    };
    if let Err(error) =
        replay_local_events_after_archive(state, request, signal, replay.watermark, projection)
    {
        return ArchiveQueryStatus::Unavailable(format!(
            "local hot data could not be joined to the archive: {error}"
        ));
    }
    ArchiveQueryStatus::Ready(replay)
}

fn replay_local_events_after_archive(
    state: &ServiceState,
    request: &QueryRequestV1,
    signal: SignalKind,
    mut after: u64,
    projection: &dyn projection::Projection,
) -> Result<()> {
    loop {
        let events = state.journal().query(EventQuery {
            signal: Some(signal),
            after,
            limit: 10_000,
        })?;
        let Some(last) = events.last() else {
            break;
        };
        after = last.cursor;
        for event in events {
            if event.event.project != request.project
                || request
                    .environment
                    .as_ref()
                    .is_some_and(|environment| event.event.environment != *environment)
            {
                continue;
            }
            let occurred = DateTime::parse_from_rfc3339(&event.event.occurred_at)
                .context("local event occurred_at must be RFC3339")?
                .with_timezone(&Utc);
            let start = request
                .time_range
                .start
                .as_deref()
                .map(DateTime::parse_from_rfc3339)
                .transpose()?
                .map(|value| value.with_timezone(&Utc));
            let end = request
                .time_range
                .end
                .as_deref()
                .map(DateTime::parse_from_rfc3339)
                .transpose()?
                .map(|value| value.with_timezone(&Utc));
            if start.is_some_and(|start| occurred < start) || end.is_some_and(|end| occurred >= end)
            {
                continue;
            }
            projection.apply_idempotent(&event)?;
        }
    }
    Ok(())
}

#[utoipa::path(
    get,
    path = "/admin/backup",
    responses(
        (status = 200, description = "exact durable-journal snapshot bytes"),
        (status = 403, description = "wildcard admin role required", body = ErrorEnvelope),
        (status = 500, description = "snapshot serialization failed", body = ErrorEnvelope)
    )
)]
async fn admin_backup(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
) -> Result<Response, ApiError> {
    authorize_global_admin(principal.as_ref().map(|principal| &principal.0))?;
    let snapshot = state
        .journal()
        .snapshot_bytes()
        .map_err(|error| ApiError::internal(format!("create durable journal snapshot: {error}")))?;
    tracing::info!(
        event = "backup_started",
        subject = principal
            .as_ref()
            .and_then(|principal| principal.0.subject())
            .unwrap_or("open-auth"),
        bytes = snapshot.len(),
        "durable journal snapshot exported"
    );
    let mut response = Response::new(Body::from(snapshot));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(
            crate::journal::infrastructure::raft::snapshot_format::SNAPSHOT_CONTENT_TYPE,
        ),
    );
    Ok(response)
}

#[derive(Debug, Deserialize)]
struct IntegrityHttpQuery {
    project: String,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IntegritySignalV1 {
    pub count: u64,
    pub watermark: u64,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IntegritySignalsV1 {
    pub logs: IntegritySignalV1,
    pub metrics: IntegritySignalV1,
    pub traces: IntegritySignalV1,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IntegrityWatermarksV1 {
    pub logs: u64,
    pub metrics: u64,
    pub traces: u64,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IntegrityWalBytesV1 {
    pub logs: u64,
    pub metrics: u64,
    pub traces: u64,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IntegrityArchiveV1 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed_at: Option<String>,
    pub watermarks: IntegrityWatermarksV1,
    pub retention_generation: u64,
    pub retention_scan_pending: bool,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IntegrityStorageV1 {
    pub wal_bytes: IntegrityWalBytesV1,
    pub archive: IntegrityArchiveV1,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct IntegrityReportV1 {
    pub version: u16,
    pub project: String,
    pub cluster_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_from: Option<String>,
    pub event_count: u64,
    pub event_id_digest_algorithm: String,
    pub event_id_sha256: String,
    pub watermark: u64,
    pub signals: IntegritySignalsV1,
    pub storage: IntegrityStorageV1,
}

impl IntegritySignalsV1 {
    fn include(&mut self, event: &StoredEvent) {
        let signal = match event.event.signal {
            SignalKind::Log => &mut self.logs,
            SignalKind::Metric => &mut self.metrics,
            SignalKind::Span => &mut self.traces,
        };
        signal.count = signal.count.saturating_add(1);
        signal.watermark = signal.watermark.max(event.cursor);
    }
}

#[utoipa::path(
    get,
    path = "/admin/integrity",
    params(("project" = String, Query, description = "project to verify")),
    responses(
        (status = 200, description = "project count, ID digest, and watermarks", body = IntegrityReportV1),
        (status = 403, description = "wildcard admin role required", body = ErrorEnvelope)
    )
)]
async fn admin_integrity(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Query(query): Query<IntegrityHttpQuery>,
) -> Result<Json<IntegrityReportV1>, ApiError> {
    authorize_global_admin(principal.as_ref().map(|principal| &principal.0))?;
    let project = query.project.trim();
    if project.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_project",
            "integrity project must not be empty",
        ));
    }

    let layout_path = state.journal().storage().root().join("layout.json");
    let layout: storage::LayoutManifest = serde_json::from_slice(
        &std::fs::read(&layout_path)
            .map_err(|error| ApiError::internal(format!("read integrity layout: {error}")))?,
    )
    .map_err(|error| ApiError::internal(format!("decode integrity layout: {error}")))?;
    let storage_root = state.journal().storage().root();
    let archive = storage::archive::committed_status(storage_root)
        .map_err(|error| ApiError::internal(format!("read archive integrity: {error}")))?;
    let watermarks = archive
        .as_ref()
        .map(|status| status.watermarks)
        .unwrap_or_default();
    let wal_bytes = |signal: &str| {
        std::fs::metadata(storage_root.join("wal").join(signal).join("events.framed"))
            .map(|metadata| metadata.len())
            .unwrap_or(0)
    };

    let mut reader = state
        .journal
        .projection_read_session(0)
        .map_err(|error| ApiError::internal(format!("open integrity scan: {error}")))?;
    let mut event_count = 0_u64;
    let mut watermark = 0_u64;
    let mut event_id_digest = [0_u8; 32];
    let mut signals = IntegritySignalsV1::default();
    loop {
        let page = reader
            .read_next(10_000)
            .map_err(|error| ApiError::internal(format!("scan integrity events: {error}")))?;
        if page.is_empty() {
            break;
        }
        for event in page.iter().filter(|event| event.event.project == project) {
            event_count = event_count.saturating_add(1);
            watermark = watermark.max(event.cursor);
            signals.include(event);
            let digest: [u8; 32] = Sha256::digest(event.event.event_id.as_bytes()).into();
            for (slot, byte) in event_id_digest.iter_mut().zip(digest) {
                *slot ^= byte;
            }
        }
    }

    Ok(Json(IntegrityReportV1 {
        version: 1,
        project: project.to_string(),
        cluster_id: layout.cluster_id,
        restored_from: layout.restored_from,
        event_count,
        event_id_digest_algorithm: "xor-sha256-v1".to_string(),
        event_id_sha256: hex::encode(event_id_digest),
        watermark,
        signals,
        storage: IntegrityStorageV1 {
            wal_bytes: IntegrityWalBytesV1 {
                logs: wal_bytes("logs"),
                metrics: wal_bytes("metrics"),
                traces: wal_bytes("traces"),
            },
            archive: IntegrityArchiveV1 {
                manifest_uri: archive.as_ref().map(|status| status.manifest_uri.clone()),
                manifest_sha256: archive
                    .as_ref()
                    .map(|status| status.manifest_sha256.clone()),
                committed_at: archive.as_ref().map(|status| status.committed_at.clone()),
                watermarks: IntegrityWatermarksV1 {
                    logs: watermarks.logs,
                    metrics: watermarks.metrics,
                    traces: watermarks.traces,
                },
                retention_generation: archive
                    .as_ref()
                    .map(|status| status.retention_generation)
                    .unwrap_or_default(),
                retention_scan_pending: archive
                    .as_ref()
                    .is_some_and(|status| status.retention_scan_pending),
            },
        },
    }))
}

fn authorize_project(principal: Option<&RoleMapPrincipal>, project: &str) -> Result<(), ApiError> {
    authorize_project_role(principal, project, Role::Write)
}

fn authorize_project_read(
    principal: Option<&RoleMapPrincipal>,
    project: &str,
) -> Result<(), ApiError> {
    authorize_project_role(principal, project, Role::Read)
}

fn authorize_global_admin(principal: Option<&RoleMapPrincipal>) -> Result<(), ApiError> {
    match principal {
        None | Some(RoleMapPrincipal::Open) => Ok(()),
        Some(principal) => principal.ensure("*", Role::Admin).map_err(|denied| {
            ApiError::forbidden(format!(
                "subject `{}` lacks wildcard admin access required for this admin operation",
                denied.subject
            ))
        }),
    }
}

fn authorize_project_role(
    principal: Option<&RoleMapPrincipal>,
    project: &str,
    role: Role,
) -> Result<(), ApiError> {
    match principal {
        None | Some(RoleMapPrincipal::Open) => Ok(()),
        Some(principal) => principal.ensure(project, role).map_err(|denied| {
            ApiError::forbidden(format!(
                "subject `{}` lacks {:?} access to project `{}`",
                denied.subject, denied.needed, denied.resource
            ))
        }),
    }
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
