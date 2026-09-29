//! Compacting the resident journal on every voter, or on a quorum without GC.

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::archive::application::run_lifecycle_attempt::prepare_lifecycle_checkpoint;
use crate::journal::infrastructure::durable_journal::DurableJournal;

pub(in crate::archive) async fn compact_remote_quorum_without_gc(
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

pub(in crate::archive) async fn compact_resident_all_voters(
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
