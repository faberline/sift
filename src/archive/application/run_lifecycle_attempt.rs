//! One lifecycle attempt: archive, checkpoint and collect, and what it
//! committed.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::journal::infrastructure::durable_journal::DurableJournal;

pub(in crate::archive) struct LifecycleCommit {
    pub(in crate::archive) manifest_uri: Option<String>,
    pub(in crate::archive) event_count: u64,
    pub(in crate::archive) segment_count: usize,
}

#[derive(Default)]
pub(in crate::archive) struct LifecycleOutcome {
    pub(in crate::archive) commit: Option<LifecycleCommit>,
    pub(in crate::archive) captured_applied_index: u64,
    pub(in crate::archive) pending_archive_gc: bool,
    pub(in crate::archive) retention_generation: Option<u64>,
    pub(in crate::archive) manifest_uri: Option<String>,
    pub(in crate::archive) manifest_sha256: Option<String>,
    pub(in crate::archive) retention_scan_pending: bool,
}

pub(in crate::archive) fn run_lifecycle_attempt(
    journal: &DurableJournal,
    state_machine: &crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine,
    destination: Option<&str>,
    retention_fence: Option<crate::journal::domain::retention_fence::RetentionFenceV1>,
) -> Result<LifecycleOutcome> {
    crate::archive::application::reconcile_committed_retention::reconcile_live_committed_retention(
        journal,
    )?;
    crate::archive::application::archive_journal_to_gcs::reconcile_committed_wal(journal)?;
    crate::archive::application::resume_local_blob_gc::resume_local_blob_gc_batch(
        journal, 128, 1_280_000,
    )?;
    let (applied_index, raw_cursor, segments) = state_machine.capture_archive_prefix()?;
    match destination {
        Some(destination) => {
            let committed_cursor =
                crate::archive::application::archive_status_queries::committed_status(
                    journal.storage().root(),
                )?
                .map(|status| status.snapshot_index)
                .unwrap_or_default();
            let mut commit = if retention_fence.is_none() && raw_cursor > committed_cursor {
                let receipt = crate::archive::application::archive_journal_to_gcs::archive_journal_gcs_captured(
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
            if crate::archive::application::archive_status_queries::committed_status(
                journal.storage().root(),
            )?
            .is_some()
            {
                crate::archive::application::evict_cold_segments::evict_committed_cold_segments_at(
                    journal,
                    Utc::now(),
                )?;
                if let Some(fence) = retention_fence {
                    let status =
                        crate::archive::application::archive_status_queries::committed_status(
                            journal.storage().root(),
                        )?
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
            let status = crate::archive::application::archive_status_queries::committed_status(
                journal.storage().root(),
            )?;
            Ok(LifecycleOutcome {
                commit,
                captured_applied_index: applied_index,
                pending_archive_gc:
                    crate::archive::infrastructure::archive_gc_pending_store::archive_gc_pending(
                        journal.storage().root(),
                    ),
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
                crate::archive::application::archive_status_queries::local_committed_watermarks(
                    journal.storage().root(),
                )?
                .max_cursor();
            let commit = if raw_cursor > committed_cursor {
                let receipt = crate::archive::application::archive_journal_local::archive_journal_local_captured(
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

pub(in crate::archive) fn prepare_lifecycle_checkpoint(
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
            let committed_cursor =
                crate::archive::application::archive_status_queries::committed_status(
                    journal.storage().root(),
                )?
                .map(|status| status.snapshot_index)
                .unwrap_or_default();
            if raw_cursor > committed_cursor {
                crate::archive::application::archive_journal_to_gcs::archive_journal_gcs_captured(
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
                crate::archive::application::archive_status_queries::local_committed_watermarks(
                    journal.storage().root(),
                )?
                .max_cursor();
            if raw_cursor > committed_cursor {
                crate::archive::application::archive_journal_local::archive_journal_local_captured(
                    journal, raw_cursor, segments,
                )?;
            }
            state_machine.prepare_local_checkpoint(applied_index, raw_cursor)?;
        }
    }
    Ok(applied_index)
}
