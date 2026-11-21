//! Committing a local archive of the journal's sealed segments.

use crate::archive::application::archive_services::ArchiveServices;

use std::collections::BTreeSet;

use anyhow::{bail, Result};
use chrono::Utc;

use crate::archive::domain::control_paths::{LOCAL_COMMIT_FORMAT_VERSION, LOCAL_COMMIT_PATH};
use crate::archive::domain::control_values::{
    expired_cursor_count, LocalCommittedSegment, LocalSegmentCommitState,
};
use crate::shared_kernel::archive_receipts::LocalArchiveReceipt;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::shared_kernel::segment_manifest::SegmentManifest;
use crate::SignalKind;

/// Seal local immutable segments and commit their exact manifest set before
/// compacting the corresponding WAL. This is the durable fallback for local
/// installations without GCS.
pub(crate) fn archive_journal_local(
    services: &ArchiveServices,
    journal: &crate::journal::application::journal_service::JournalService<'_>,
) -> Result<LocalArchiveReceipt> {
    let (snapshot_index, segments) = journal.seal_archive_prefix()?;
    archive_journal_local_captured(services, journal, snapshot_index, segments)
}

/// Checks that the sealed cursors plus the expired ranges are exactly 1..=snapshot_index.
pub(crate) fn check_local_prefix(
    cursors: &BTreeSet<u64>,
    expired: &[(u64, u64)],
    event_count: u64,
    snapshot_index: u64,
) -> Result<()> {
    for &(first, last) in expired {
        if let Some(&cursor) = cursors.range(first..=last).next() {
            bail!("local archive contains duplicate cursor {cursor}");
        }
    }
    let covered_by = |cursor: u64| {
        expired
            .iter()
            .find(|(first, last)| *first <= cursor && cursor <= *last)
            .map(|range| range.1)
    };
    let mut expected = 1_u64;
    let mut present = cursors.iter().copied().peekable();
    loop {
        if let Some(last) = covered_by(expected) {
            expected = last.saturating_add(1);
            continue;
        }
        match present.peek().copied() {
            Some(cursor) if cursor == expected => {
                present.next();
                expected = expected.saturating_add(1);
            }
            Some(cursor) => {
                bail!("local archive prefix expected cursor {expected}, found {cursor}");
            }
            None => break,
        }
    }
    if event_count.saturating_add(expired_cursor_count(expired)) != snapshot_index {
        bail!(
            "local archive contains {event_count} events but captured cursor is {snapshot_index}"
        );
    }
    if expected - 1 < snapshot_index {
        bail!("local archive prefix expected cursor {expected}, found nothing");
    }
    Ok(())
}

#[doc(hidden)]
pub(crate) fn archive_journal_local_captured(
    services: &ArchiveServices,
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    snapshot_index: u64,
    segments: Vec<(SignalKind, SegmentManifest)>,
) -> Result<LocalArchiveReceipt> {
    let expired = services
        .read_local_commit_state(journal.storage().root())?
        .map(|state| state.expired_cursors)
        .unwrap_or_default();
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
    check_local_prefix(&cursors, &expired, event_count, snapshot_index)?;
    committed.sort_by_key(|segment| segment.manifest.first_cursor);
    let committed_at = Utc::now().to_rfc3339();
    let state = LocalSegmentCommitState {
        format_version: LOCAL_COMMIT_FORMAT_VERSION,
        committed_at: committed_at.clone(),
        snapshot_index,
        watermarks,
        segments: committed,
        expired_cursors: expired,
    };
    let path = journal.storage().root().join(LOCAL_COMMIT_PATH);
    storage_durable::atomic_write(
        &path,
        &serde_json::to_vec_pretty(&state)?,
        storage_durable::FsyncPolicy::Always,
    )?;
    services.set_private_file(&path)?;
    journal.compact_archived_wal(watermarks)?;
    Ok(LocalArchiveReceipt {
        committed_at,
        snapshot_index,
        watermarks,
        event_count,
        segment_count: state.segments.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::check_local_prefix;
    use std::collections::BTreeSet;

    fn set(values: &[u64]) -> BTreeSet<u64> {
        values.iter().copied().collect()
    }

    #[test]
    fn check_local_prefix_accepts_a_listed_gap() {
        assert!(check_local_prefix(&set(&[1, 2, 3, 7, 8]), &[(4, 6)], 5, 8).is_ok());
    }

    #[test]
    fn check_local_prefix_rejects_an_unlisted_gap() {
        let error = check_local_prefix(&set(&[1, 2, 3, 7, 8]), &[], 5, 8).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("local archive prefix expected cursor 4, found 7"),
            "{error}"
        );
    }

    #[test]
    fn check_local_prefix_rejects_a_wrong_event_count() {
        assert!(check_local_prefix(&set(&[1, 2, 3]), &[], 3, 4).is_err());
    }

    #[test]
    fn check_local_prefix_accepts_no_expiry() {
        assert!(check_local_prefix(&set(&[1, 2, 3]), &[], 3, 3).is_ok());
    }

    #[test]
    fn check_local_prefix_accepts_an_expired_tail() {
        assert!(check_local_prefix(&set(&[1, 2]), &[(3, 5)], 2, 5).is_ok());
    }

    #[test]
    fn check_local_prefix_rejects_an_overlap_of_expired_and_present_cursors() {
        let error = check_local_prefix(&set(&[1, 2, 3]), &[(3, 4)], 3, 4).unwrap_err();
        assert!(error.to_string().contains("duplicate cursor 3"), "{error}");
    }
}
