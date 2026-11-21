//! Archive control records. Field order and serde contracts are unchanged.

use crate::shared_kernel::archive_manifest::ArchiveManifest;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::shared_kernel::catalog_manifest::CatalogRoot;
use crate::shared_kernel::segment_manifest::SegmentManifest;
use crate::{ContentBlobRef, SignalKind};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArchiveCommitState {
    pub(in crate::archive) format_version: u16,
    pub(in crate::archive) manifest_uri: String,
    pub(in crate::archive) manifest_sha256: String,
    pub(in crate::archive) committed_at: String,
    pub(in crate::archive) watermarks: ArchiveWatermarks,
    pub(in crate::archive) manifest: ArchiveManifest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArchiveUploadIntent {
    pub(in crate::archive) format_version: u16,
    pub(in crate::archive) destination_uri: String,
    pub(in crate::archive) archive_prefix: String,
    pub(in crate::archive) manifest_uri: String,
    pub(in crate::archive) generated_at: String,
    pub(in crate::archive) captured_cursor: u64,
    pub(in crate::archive) source_cluster_id: String,
    pub(in crate::archive) source_manifest_uri: Option<String>,
    pub(in crate::archive) source_manifest_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArchiveGcPending {
    pub(in crate::archive) format_version: u16,
    pub(in crate::archive) replacement_manifest_uri: String,
    pub(in crate::archive) replacement_manifest_sha256: String,
    pub(in crate::archive) gc_plan_uri: String,
    pub(in crate::archive) gc_plan_root: CatalogRoot,
    pub(in crate::archive) cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalBlobGcPending {
    pub(in crate::archive) format_version: u16,
    pub(in crate::archive) replacement_manifest_uri: String,
    pub(in crate::archive) replacement_manifest_sha256: String,
    pub(in crate::archive) gc_plan_uri: String,
    pub(in crate::archive) gc_plan_root: CatalogRoot,
    pub(in crate::archive) plan_cursor: Option<String>,
    pub(in crate::archive) plan_exhausted: bool,
    pub(in crate::archive) candidates: Vec<String>,
    pub(in crate::archive) scan_start_cursor: u64,
    pub(in crate::archive) scanned_through_cursor: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalSegmentCommitState {
    pub(in crate::archive) format_version: u16,
    pub(in crate::archive) committed_at: String,
    pub(in crate::archive) snapshot_index: u64,
    pub(in crate::archive) watermarks: ArchiveWatermarks,
    pub(in crate::archive) segments: Vec<LocalCommittedSegment>,
    /// Inclusive cursor ranges that local expiry removed: sorted, non-overlapping, non-adjacent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in crate::archive) expired_cursors: Vec<(u64, u64)>,
}

/// Merges inclusive cursor ranges into sorted, non-overlapping, non-adjacent
/// ranges. This is the only place that builds the expired ranges of a commit.
pub(crate) fn merge_expired_ranges(
    existing: &[(u64, u64)],
    added: &[(u64, u64)],
) -> Vec<(u64, u64)> {
    let mut all: Vec<(u64, u64)> = existing
        .iter()
        .chain(added.iter())
        .copied()
        .filter(|(first, last)| first <= last)
        .collect();
    all.sort_by_key(|range| range.0);
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(all.len());
    for (first, last) in all {
        match merged.last_mut() {
            Some(current) if first <= current.1.saturating_add(1) => {
                current.1 = current.1.max(last);
            }
            _ => merged.push((first, last)),
        }
    }
    merged
}

/// The number of cursors in inclusive ranges.
pub(crate) fn expired_cursor_count(ranges: &[(u64, u64)]) -> u64 {
    ranges.iter().fold(0_u64, |total, &(first, last)| {
        total.saturating_add(last.saturating_sub(first).saturating_add(1))
    })
}

/// The highest cursor that a local commit covers, including an expired tail.
pub(crate) fn highest_committed_cursor(
    watermarks: &ArchiveWatermarks,
    expired: &[(u64, u64)],
) -> u64 {
    watermarks
        .max_cursor()
        .max(expired.last().map(|range| range.1).unwrap_or(0))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalCommittedSegment {
    pub(in crate::archive) signal: SignalKind,
    pub(in crate::archive) manifest: SegmentManifest,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RestorePhase {
    Building,
    Ready,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestoreState {
    pub(in crate::archive) format_version: u16,
    pub(in crate::archive) manifest_uri: String,
    pub(in crate::archive) phase: RestorePhase,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct SpillBlobReference {
    pub(in crate::archive) reference: ContentBlobRef,
    pub(in crate::archive) count: u64,
}

#[cfg(test)]
mod tests {
    use super::{
        expired_cursor_count, highest_committed_cursor, merge_expired_ranges,
        LocalSegmentCommitState,
    };
    use crate::archive::domain::control_paths::LOCAL_COMMIT_FORMAT_VERSION;
    use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
    use crate::SignalKind;

    #[test]
    fn merge_expired_ranges_merges_adjacent() {
        assert_eq!(merge_expired_ranges(&[(1, 3)], &[(4, 6)]), vec![(1, 6)]);
    }

    #[test]
    fn merge_expired_ranges_merges_overlapping() {
        assert_eq!(merge_expired_ranges(&[(1, 5)], &[(3, 8)]), vec![(1, 8)]);
    }

    #[test]
    fn merge_expired_ranges_sorts_unsorted_input() {
        assert_eq!(
            merge_expired_ranges(&[(10, 12), (1, 2)], &[(5, 6)]),
            vec![(1, 2), (5, 6), (10, 12)]
        );
    }

    #[test]
    fn merge_expired_ranges_of_nothing_is_empty() {
        assert_eq!(merge_expired_ranges(&[], &[]), Vec::<(u64, u64)>::new());
    }

    #[test]
    fn expired_cursor_count_sums_inclusive_ranges() {
        assert_eq!(expired_cursor_count(&[(1, 3), (10, 10)]), 4);
    }

    #[test]
    fn highest_committed_cursor_counts_an_expired_tail() {
        let mut watermarks = ArchiveWatermarks::default();
        watermarks.include(SignalKind::Log, 90);
        assert_eq!(highest_committed_cursor(&watermarks, &[(91, 100)]), 100);
        assert_eq!(highest_committed_cursor(&watermarks, &[]), 90);
    }

    #[test]
    fn commit_state_without_expired_cursors_reads_an_empty_list() {
        let value = serde_json::json!({
            "format_version": LOCAL_COMMIT_FORMAT_VERSION,
            "committed_at": "2026-01-01T00:00:00Z",
            "snapshot_index": 0,
            "watermarks": serde_json::to_value(ArchiveWatermarks::default()).unwrap(),
            "segments": [],
        });
        let state: LocalSegmentCommitState = serde_json::from_value(value).unwrap();
        assert!(state.expired_cursors.is_empty());
        let written = serde_json::to_value(&state).unwrap();
        assert!(written.get("expired_cursors").is_none());
    }

    #[test]
    fn commit_state_with_expired_cursors_writes_them() {
        let mut state: LocalSegmentCommitState = serde_json::from_value(serde_json::json!({
            "format_version": LOCAL_COMMIT_FORMAT_VERSION,
            "committed_at": "2026-01-01T00:00:00Z",
            "snapshot_index": 0,
            "watermarks": serde_json::to_value(ArchiveWatermarks::default()).unwrap(),
            "segments": [],
        }))
        .unwrap();
        state.expired_cursors = vec![(2, 3)];
        let written = serde_json::to_value(&state).unwrap();
        assert_eq!(written["expired_cursors"], serde_json::json!([[2, 3]]));
    }
}
