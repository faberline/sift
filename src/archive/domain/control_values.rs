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
