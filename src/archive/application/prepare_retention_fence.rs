//! Preparing the retention fence a lifecycle pass carries, and clearing it once
//! done.

use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::journal::infrastructure::durable_journal::DurableJournal;

pub(in crate::archive) async fn prepare_retention_fence(
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
        if let Some(status) = crate::archive::application::archive_status_queries::committed_status(
            journal.storage().root(),
        )? {
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
    if !crate::archive::application::retention_due::retention_due_at(
        journal.storage().root(),
        evaluate_at,
    )? {
        return Ok(None);
    }
    let status = crate::archive::application::archive_status_queries::committed_status(
        journal.storage().root(),
    )?
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

pub(in crate::archive) async fn clear_completed_retention_fence(
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
