//! One lifecycle tick on the leader: wait for every voter's snapshot
//! capability, prepare the retention fence, run the archive attempt, then
//! checkpoint, collect garbage and reconcile local capacity.

use crate::archive::application::compact_journal::compact_resident_all_voters;
use crate::archive::application::lifecycle_checkpoint::{
    propose_no_gc_quorum_barrier, propose_retention_barrier, run_all_voter_checkpoint,
    run_archive_gc_after_checkpoint, run_no_gc_quorum_checkpoint,
};
use crate::archive::application::prepare_retention_fence::prepare_retention_fence;
use crate::archive::application::run_lifecycle_attempt::{run_lifecycle_attempt, LifecycleContext};

/// What a tick's checkpoint steps report back: whether local state changed,
/// so capacity is reconciled, and whether a checkpoint reached every voter,
/// which archive GC on a replicated store waits for.
pub(super) struct TickFlags {
    pub(super) changed: bool,
    pub(super) replicated_checkpoint_installed: bool,
}

/// Run one lifecycle tick. The worker calls it only while this node leads.
pub(in crate::archive) async fn run_lifecycle_tick(ctx: &LifecycleContext) {
    let remote_archive = ctx.destination.is_some();
    let replicated = ctx.raft.is_some();
    let retention_capable = if remote_archive && replicated {
        match ctx
            .raft
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
        &ctx.journal,
        &ctx.state_machine,
        ctx.raft.as_ref(),
        remote_archive,
        retention_capable,
    )
    .await;
    let archive = match retention_fence {
        Ok(retention_fence) => {
            let attempt_journal = ctx.journal.clone();
            let attempt_state_machine = ctx.state_machine.clone();
            let attempt_destination = ctx.destination.clone();
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
            let mut flags = TickFlags {
                changed: outcome.commit.is_some(),
                replicated_checkpoint_installed: false,
            };
            let still_leader = match &ctx.raft {
                Some(raft) => raft.is_leader().await,
                None => true,
            };
            if !still_leader {
                tracing::warn!(
                    "Sift lifecycle lost leadership after archive work; checkpoint and GC are deferred"
                );
            }
            if let Some(raft) = &ctx.raft {
                let mut checkpoint_allowed = still_leader && outcome.captured_applied_index > 0;
                if outcome.retention_scan_pending {
                    checkpoint_allowed = false;
                    tracing::debug!(
                        "Sift keeps the retention fence and Raft log until the bounded scan completes"
                    );
                }
                let mut quorum_only_checkpoint = false;
                if checkpoint_allowed && remote_archive && outcome.pending_archive_gc {
                    if !retention_capable {
                        checkpoint_allowed = false;
                        propose_no_gc_quorum_barrier(
                            raft,
                            &mut outcome,
                            &mut quorum_only_checkpoint,
                        )
                        .await;
                    } else {
                        propose_retention_barrier(raft, &mut outcome, &mut checkpoint_allowed)
                            .await;
                    }
                }
                if checkpoint_allowed {
                    run_all_voter_checkpoint(ctx, raft, &outcome, remote_archive, &mut flags).await;
                }
                if quorum_only_checkpoint {
                    run_no_gc_quorum_checkpoint(ctx, raft, &outcome, &mut flags).await;
                }
            }
            let archive_gc_is_safe = still_leader
                && remote_archive
                && !outcome.retention_scan_pending
                && (!replicated || flags.replicated_checkpoint_installed);
            if archive_gc_is_safe {
                run_archive_gc_after_checkpoint(ctx).await;
            }
            if flags.changed {
                if let Err(error) = ctx.local_capacity.reconcile() {
                    tracing::warn!(
                        %error,
                        "Sift lifecycle committed but capacity reconciliation failed"
                    );
                }
            }
            if let Some(receipt) = outcome.commit {
                tracing::info!(
                    manifest_uri = receipt.manifest_uri.as_deref().unwrap_or("local"),
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
            if let Some(raft) = &ctx.raft {
                if raft.is_leader().await {
                    if let Err(fallback_error) =
                        compact_resident_all_voters(&ctx.state_machine, raft).await
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
