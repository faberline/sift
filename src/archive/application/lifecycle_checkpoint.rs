//! The lifecycle's Raft checkpoint steps after an archive attempt: the
//! checkpoint barriers it proposes, the all-voter checkpoint and its
//! fallbacks, the no-GC quorum checkpoint, and the archive GC a checkpoint on
//! every voter allows.

use std::sync::Arc;

use crate::archive::application::compact_journal::{
    compact_remote_quorum_without_gc, compact_resident_all_voters,
};
use crate::archive::application::lifecycle_tick::TickFlags;
use crate::archive::application::prepare_retention_fence::clear_completed_retention_fence;
use crate::archive::application::run_lifecycle_attempt::{
    prepare_lifecycle_checkpoint, LifecycleContext, LifecycleOutcome,
};
use crate::archive::domain::lifecycle_settings::{
    ALL_VOTER_CHECKPOINT_ATTEMPT, ARCHIVE_GC_BATCH_OBJECTS,
};

/// Without every voter's snapshot capability, propose a checkpoint barrier
/// for a quorum-only checkpoint that retains every prior archive object.
pub(super) async fn propose_no_gc_quorum_barrier(
    raft: &Arc<crate::journal::application::raft_checkpoint_service::RaftCheckpointService>,
    outcome: &mut LifecycleOutcome,
    quorum_only_checkpoint: &mut bool,
) {
    if let (Some(retention_generation), Some(manifest_uri), Some(manifest_sha256)) = (
        outcome.retention_generation,
        outcome.manifest_uri.clone(),
        outcome.manifest_sha256.clone(),
    ) {
        match raft.encode(
            &crate::shared_kernel::sift_command::SiftCommandV1::ArchiveCheckpointBarrier {
                retention_generation,
                manifest_uri,
                manifest_sha256,
            },
        ) {
            Ok(command) => match raft.propose(command).await {
                Ok(index) => {
                    outcome.captured_applied_index = index;
                    *quorum_only_checkpoint = true;
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
        tracing::warn!("Sift archive GC is pending without a checkpoint identity");
    }
}

/// Propose the retention barrier the all-voter checkpoint covers. A failed
/// proposal, or an outcome without a retention generation, disallows the
/// checkpoint.
pub(super) async fn propose_retention_barrier(
    raft: &Arc<crate::journal::application::raft_checkpoint_service::RaftCheckpointService>,
    outcome: &mut LifecycleOutcome,
    checkpoint_allowed: &mut bool,
) {
    if let (Some(retention_generation), Some(manifest_uri), Some(manifest_sha256)) = (
        outcome.retention_generation,
        outcome.manifest_uri.clone(),
        outcome.manifest_sha256.clone(),
    ) {
        match raft.encode(
            &crate::shared_kernel::sift_command::SiftCommandV1::ArchiveCheckpointBarrier {
                retention_generation,
                manifest_uri,
                manifest_sha256,
            },
        ) {
            Ok(command) => match raft.propose(command).await {
                Ok(index) => {
                    outcome.captured_applied_index = index;
                }
                Err(error) => {
                    *checkpoint_allowed = false;
                    tracing::warn!(
                        %error,
                        "Sift retention barrier proposal failed"
                    );
                }
            },
            Err(error) => {
                *checkpoint_allowed = false;
                tracing::warn!(
                    %error,
                    "Sift retention barrier encoding failed"
                );
            }
        }
    } else {
        *checkpoint_allowed = false;
        tracing::warn!("Sift archive GC is pending without a retention generation");
    }
}

/// Prepare a checkpoint and install it on every voter. If the install fails
/// on a remote archive, compact on a quorum without GC; if the preparation
/// fails, fall back to a resident checkpoint on every voter.
pub(super) async fn run_all_voter_checkpoint(
    ctx: &LifecycleContext,
    raft: &Arc<crate::journal::application::raft_checkpoint_service::RaftCheckpointService>,
    outcome: &LifecycleOutcome,
    remote_archive: bool,
    flags: &mut TickFlags,
) {
    let services = ctx.services.clone();
    let checkpoint_journal = ctx.journal.clone();
    let checkpoint_state_machine = ctx.state_machine.clone();
    let checkpoint_destination = ctx.destination.clone();
    let prepared = tokio::task::spawn_blocking(move || {
        prepare_lifecycle_checkpoint(
            &services,
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
                    flags.changed = true;
                    flags.replicated_checkpoint_installed = true;
                    if let Some(retention_generation) = outcome.retention_generation {
                        if let Err(error) = clear_completed_retention_fence(
                            raft,
                            &ctx.state_machine,
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
                        compacted_raft_index = compaction.snapshot_index,
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
                            ctx.services.clone(),
                            &ctx.journal,
                            &ctx.state_machine,
                            &ctx.destination,
                            raft,
                        )
                        .await
                        {
                            Ok(compaction) if compaction.installed => {
                                flags.changed = true;
                                if let Some(retention_generation) = outcome.retention_generation {
                                    if let Err(error) = clear_completed_retention_fence(
                                        raft,
                                        &ctx.state_machine,
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
            if let Err(fallback_error) = compact_resident_all_voters(&ctx.state_machine, raft).await
            {
                tracing::warn!(
                    %fallback_error,
                    "Sift resident all-voter checkpoint failed"
                );
            } else {
                flags.changed = true;
            }
        }
        Err(error) => tracing::warn!(
            %error,
            "Sift checkpoint preparation task panicked"
        ),
    }
}

/// Install the no-GC checkpoint the quorum-only barrier proposed.
pub(super) async fn run_no_gc_quorum_checkpoint(
    ctx: &LifecycleContext,
    raft: &Arc<crate::journal::application::raft_checkpoint_service::RaftCheckpointService>,
    outcome: &LifecycleOutcome,
    flags: &mut TickFlags,
) {
    match compact_remote_quorum_without_gc(
        ctx.services.clone(),
        &ctx.journal,
        &ctx.state_machine,
        &ctx.destination,
        raft,
    )
    .await
    {
        Ok(compaction) if compaction.installed => {
            flags.changed = true;
            if let Some(retention_generation) = outcome.retention_generation {
                if let Err(error) =
                    clear_completed_retention_fence(raft, &ctx.state_machine, retention_generation)
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

/// Finish local blob GC and delete one batch of obsolete archive objects once
/// a checkpoint allows it.
pub(super) async fn run_archive_gc_after_checkpoint(ctx: &LifecycleContext) {
    let services = ctx.services.clone();
    let gc_journal = ctx.journal.clone();
    match tokio::task::spawn_blocking(move || {
        crate::archive::application::resume_local_blob_gc::finish_local_blob_gc(&services, &gc_journal)?;
        crate::archive::application::finalize_archive_gc::finalize_archive_gc_batch_after_checkpoint(&services,
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
