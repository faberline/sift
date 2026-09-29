//! The leader's lifecycle loop: archive, evict, expire and collect on each
//! tick.

use crate::app::service_state::ServiceState;
use crate::archive::application::compact_journal::{
    compact_remote_quorum_without_gc, compact_resident_all_voters,
};
use crate::archive::application::prepare_retention_fence::{
    clear_completed_retention_fence, prepare_retention_fence,
};
use crate::archive::application::run_lifecycle_attempt::{
    prepare_lifecycle_checkpoint, run_lifecycle_attempt,
};
use crate::archive::domain::lifecycle_settings::{
    ALL_VOTER_CHECKPOINT_ATTEMPT, ARCHIVE_GC_BATCH_OBJECTS,
};
use crate::archive::interfaces::archive_worker::ArchiveWorker;

impl ServiceState {
    pub(super) fn start_lifecycle_worker(
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
                                    crate::archive::application::resume_local_blob_gc::finish_local_blob_gc(&gc_journal)?;
                                    crate::archive::application::finalize_archive_gc::finalize_archive_gc_batch_after_checkpoint(
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
