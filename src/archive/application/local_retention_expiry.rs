//! Local retention expiry: removes local events older than the retention
//! window from the sealed local segments and rewrites the local commit state.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::archive::application::archive_services::ArchiveServices;
use crate::archive::application::retention_expiration::retention_cutoffs;
use crate::archive::domain::control_paths::{LOCAL_COMMIT_FORMAT_VERSION, LOCAL_COMMIT_PATH};
use crate::archive::domain::control_values::{
    merge_expired_ranges, LocalCommittedSegment, LocalSegmentCommitState,
};
use crate::journal::application::journal_service::JournalService;
use crate::shared_kernel::archive_receipts::LocalExpiryReceipt;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::shared_kernel::retention_boundary::RetentionDays;

/// Removes local events older than `retention` days before `now`, from the
/// committed prefix only, and rewrites the local commit state to match.
///
/// Order: evict first, then write the commit. A rewritten segment gets a new
/// segment id that exists only after the eviction, and `read_local_commit_state`
/// rejects a commit whose manifests differ from the files. A crash between the
/// two steps leaves the local archive unable to commit until the next call of
/// this function repairs it: the gaps in the cursors are read again (step 6).
/// The caller must run this before `archive_journal_local_captured` in a tick.
pub(crate) fn expire_local_events_at(
    services: &ArchiveServices,
    journal: &JournalService<'_>,
    now: DateTime<Utc>,
    retention: RetentionDays,
) -> Result<LocalExpiryReceipt> {
    let root = journal.storage().root();
    let Some(state) = services.read_local_commit_state(root)? else {
        return Ok(LocalExpiryReceipt::default());
    };
    let (cutoff_nanos, _) = retention_cutoffs(now, retention)?;
    let cutoff = DateTime::<Utc>::from_timestamp_nanos(cutoff_nanos);

    let removed = journal
        .storage()
        .evict_expired_before(cutoff, state.snapshot_index)?;
    journal.evict_resident_before(cutoff)?;

    let mut watermarks = ArchiveWatermarks::default();
    let mut present = BTreeSet::new();
    let mut retained_events = 0_u64;
    let mut oldest_retained_unix_nano: Option<i64> = None;
    let mut committed = Vec::new();
    for (signal, manifest) in journal.storage().seal_all_with_signal()? {
        // Sealing also seals rows after the commit; they are not part of it.
        if manifest.last_cursor > state.snapshot_index {
            continue;
        }
        let events = journal.storage().read_segment_events(signal, &manifest)?;
        for event in &events {
            present.insert(event.cursor);
            retained_events = retained_events.saturating_add(1);
            let occurred = DateTime::parse_from_rfc3339(&event.event.occurred_at)
                .context("local retained event occurred_at must be RFC3339")?
                .with_timezone(&Utc);
            let nanos = occurred
                .timestamp_nanos_opt()
                .context("local retained event occurred_at is outside the nanosecond range")?;
            oldest_retained_unix_nano =
                Some(oldest_retained_unix_nano.map_or(nanos, |oldest| oldest.min(nanos)));
        }
        watermarks.include(signal, manifest.last_cursor);
        committed.push(LocalCommittedSegment { signal, manifest });
    }
    committed.sort_by_key(|segment| segment.manifest.first_cursor);

    // Gaps are read from the disk, not from the eviction count, so that a
    // crash after the eviction is repaired by the next call (step 6).
    let expired = merge_expired_ranges(
        &state.expired_cursors,
        &removed_ranges(&present, state.snapshot_index),
    );
    if removed == 0 && expired == state.expired_cursors {
        return Ok(LocalExpiryReceipt {
            removed_events: 0,
            retained_events,
            oldest_retained_unix_nano,
        });
    }
    let new_state = LocalSegmentCommitState {
        format_version: LOCAL_COMMIT_FORMAT_VERSION,
        committed_at: Utc::now().to_rfc3339(),
        snapshot_index: state.snapshot_index,
        watermarks,
        segments: committed,
        expired_cursors: expired,
    };
    let path = root.join(LOCAL_COMMIT_PATH);
    storage_durable::atomic_write(
        &path,
        &serde_json::to_vec_pretty(&new_state)?,
        storage_durable::FsyncPolicy::Always,
    )?;
    services.set_private_file(&path)?;

    Ok(LocalExpiryReceipt {
        removed_events: removed,
        retained_events,
        oldest_retained_unix_nano,
    })
}

/// The maximal runs of cursors in `1..=snapshot_index` that are not in
/// `present`, sorted, inclusive.
fn removed_ranges(present: &BTreeSet<u64>, snapshot_index: u64) -> Vec<(u64, u64)> {
    let mut ranges = Vec::new();
    let mut start: Option<u64> = None;
    for cursor in 1..=snapshot_index {
        if present.contains(&cursor) {
            if let Some(first) = start.take() {
                ranges.push((first, cursor - 1));
            }
        } else if start.is_none() {
            start = Some(cursor);
        }
    }
    if let Some(first) = start {
        ranges.push((first, snapshot_index));
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::removed_ranges;
    use std::collections::BTreeSet;

    fn set(values: &[u64]) -> BTreeSet<u64> {
        values.iter().copied().collect()
    }

    #[test]
    fn removed_ranges_of_a_full_prefix_is_empty() {
        assert_eq!(
            removed_ranges(&set(&[1, 2, 3]), 3),
            Vec::<(u64, u64)>::new()
        );
    }

    #[test]
    fn removed_ranges_finds_a_head_a_middle_and_a_tail_gap() {
        assert_eq!(
            removed_ranges(&set(&[3, 4, 8]), 10),
            vec![(1, 2), (5, 7), (9, 10)]
        );
    }

    #[test]
    fn removed_ranges_of_nothing_present_is_the_whole_prefix() {
        assert_eq!(removed_ranges(&set(&[]), 4), vec![(1, 4)]);
        assert_eq!(removed_ranges(&set(&[]), 0), Vec::<(u64, u64)>::new());
    }
}
