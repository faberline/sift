//! Committing a local archive of the journal's sealed segments.

use std::collections::BTreeSet;

use anyhow::{bail, Result};
use chrono::Utc;

use crate::archive::application::archive_receipts::LocalArchiveReceipt;
use crate::archive::infrastructure::archive_control_paths::{
    LOCAL_COMMIT_FORMAT_VERSION, LOCAL_COMMIT_PATH,
};
use crate::archive::infrastructure::local_segment_commit_state::{
    LocalCommittedSegment, LocalSegmentCommitState,
};
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::storage::SegmentManifest;
use crate::SignalKind;

#[cfg(any(not(unix), unix))]
use crate::archive::infrastructure::private_fs_mode::set_private_file;

/// Seal local immutable segments and commit their exact manifest set before
/// compacting the corresponding WAL. This is the durable fallback for local
/// installations without GCS.
pub fn archive_journal_local(
    journal: &crate::journal::infrastructure::durable_journal::DurableJournal,
) -> Result<LocalArchiveReceipt> {
    let (snapshot_index, segments) = journal.seal_archive_prefix()?;
    archive_journal_local_captured(journal, snapshot_index, segments)
}

#[doc(hidden)]
pub fn archive_journal_local_captured(
    journal: &crate::journal::infrastructure::durable_journal::DurableJournal,
    snapshot_index: u64,
    segments: Vec<(SignalKind, SegmentManifest)>,
) -> Result<LocalArchiveReceipt> {
    let mut watermarks = ArchiveWatermarks::default();
    let mut event_count = 0_u64;
    let mut cursors = BTreeSet::new();
    let mut committed = Vec::with_capacity(segments.len());
    for (signal, manifest) in segments {
        let events = journal.storage().read_segment_events(signal, &manifest)?;
        event_count = event_count.saturating_add(events.len() as u64);
        for event in &events {
            if !cursors.insert(event.cursor) {
                bail!("local archive contains duplicate cursor {}", event.cursor);
            }
        }
        watermarks.include(signal, manifest.last_cursor);
        committed.push(LocalCommittedSegment { signal, manifest });
    }
    for (offset, cursor) in cursors.into_iter().enumerate() {
        let expected = offset as u64 + 1;
        if cursor != expected {
            bail!("local archive prefix expected cursor {expected}, found {cursor}");
        }
    }
    if event_count != snapshot_index {
        bail!(
            "local archive contains {event_count} events but captured cursor is {snapshot_index}"
        );
    }
    committed.sort_by_key(|segment| segment.manifest.first_cursor);
    let committed_at = Utc::now().to_rfc3339();
    let state = LocalSegmentCommitState {
        format_version: LOCAL_COMMIT_FORMAT_VERSION,
        committed_at: committed_at.clone(),
        snapshot_index,
        watermarks,
        segments: committed,
    };
    let path = journal.storage().root().join(LOCAL_COMMIT_PATH);
    storage_durable::atomic_write(
        &path,
        &serde_json::to_vec_pretty(&state)?,
        storage_durable::FsyncPolicy::Always,
    )?;
    set_private_file(&path)?;
    journal.compact_archived_wal(watermarks)?;
    Ok(LocalArchiveReceipt {
        committed_at,
        snapshot_index,
        watermarks,
        event_count,
        segment_count: state.segments.len(),
    })
}
