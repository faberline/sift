//! Preparing the retention fence a lifecycle pass carries, and clearing it once
//! done.

use crate::archive::application::archive_services::ArchiveServices;

use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::journal::application::journal_service::JournalService as DurableJournal;

pub(in crate::archive) async fn prepare_retention_fence(
    services: &ArchiveServices,
    journal: &Arc<DurableJournal<'static>>,
    state_machine: &Arc<crate::journal::application::checkpoint_service::JournalCheckpointService>,
    raft: Option<&Arc<crate::journal::application::raft_checkpoint_service::RaftCheckpointService>>,
    remote_archive: bool,
    retention_capable: bool,
) -> Result<Option<crate::shared_kernel::retention_fence::RetentionFenceV1>> {
    if let Some((fence, applied_index)) = state_machine.pending_retention_fence() {
        if let Some(raft) = raft {
            raft.require_applied_index_on_all_voters(applied_index)
                .await
                .context("wait for every Sift voter to apply the retention fence")?;
        }
        if let Some(status) = crate::archive::application::archive_status_queries::committed_status(
            services,
            journal.storage().root(),
        )? {
            if status.retention_scan_pending
                && status.retention_generation >= fence.target_generation
            {
                let next = crate::shared_kernel::retention_fence::RetentionFenceV1 {
                    source_manifest_uri: status.manifest_uri,
                    source_manifest_sha256: status.manifest_sha256,
                    target_generation: status.retention_generation.saturating_add(1),
                    evaluate_at: fence.evaluate_at,
                };
                if let Some(raft) = raft {
                    let command = raft.encode(
                        &crate::shared_kernel::sift_command::SiftCommandV1::RetentionFence {
                            fence: next.clone(),
                        },
                    )?;
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
        services,
        journal.storage().root(),
        evaluate_at,
    )? {
        return Ok(None);
    }
    let status = crate::archive::application::archive_status_queries::committed_status(
        services,
        journal.storage().root(),
    )?
    .context("Sift retention requires a committed archive")?;
    let fence = crate::shared_kernel::retention_fence::RetentionFenceV1 {
        source_manifest_uri: status.manifest_uri,
        source_manifest_sha256: status.manifest_sha256,
        target_generation: status.retention_generation.saturating_add(1),
        evaluate_at: evaluate_at.to_rfc3339(),
    };
    if let Some(raft) = raft {
        let command = raft.encode(
            &crate::shared_kernel::sift_command::SiftCommandV1::RetentionFence {
                fence: fence.clone(),
            },
        )?;
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
    raft: &Arc<crate::journal::application::raft_checkpoint_service::RaftCheckpointService>,
    state_machine: &Arc<crate::journal::application::checkpoint_service::JournalCheckpointService>,
    retention_generation: u64,
) -> Result<()> {
    let Some((fence, _)) = state_machine.pending_retention_fence() else {
        return Ok(());
    };
    if fence.target_generation > retention_generation {
        return Ok(());
    }
    raft.propose(raft.encode(
        &crate::shared_kernel::sift_command::SiftCommandV1::clear_retention_fence(
            retention_generation,
        ),
    )?)
    .await
    .context("commit Sift retention fence clear to quorum")?;
    Ok(())
}
