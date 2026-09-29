//! The committed archive manifest and what it lists: segments, blobs, dedupe
//! receipts, and the retention delta and scan position.

use serde::{Deserialize, Serialize};

use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::storage::{EpochMap, SegmentManifest};
use crate::{ContentBlobRef, SignalKind};

pub(in crate::archive) const ARCHIVE_FORMAT_VERSION: u16 = 10;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ArchiveBlob {
    pub reference: ContentBlobRef,
    pub object_uri: String,
    #[serde(default)]
    pub reference_count: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ArchiveSegment {
    pub signal: SignalKind,
    pub source: SegmentManifest,
    pub object_uri: String,
    pub parquet_bytes: u64,
    pub parquet_sha256: String,
    /// Exact acceptance-time bounds let a dedupe rebuild skip cold segments.
    pub min_acknowledged_at_unix_nano: i64,
    pub max_acknowledged_at_unix_nano: i64,
    pub dedupe_receipt: ArchiveDedupeReceipt,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveDedupeReceipt {
    pub object_uri: String,
    pub bytes: u64,
    pub sha256: String,
    pub entry_count: u64,
    pub first_cursor: u64,
    pub last_cursor: u64,
    pub min_acknowledged_at_unix_nano: i64,
    pub max_acknowledged_at_unix_nano: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ArchiveManifest {
    pub format_version: u16,
    pub generated_at: String,
    pub source_cluster_id: String,
    pub source_node_id: String,
    pub raft_snapshot_index: u64,
    pub event_count: u64,
    pub event_id_digest_algorithm: String,
    pub event_id_sha256: String,
    pub event_content_digest_algorithm: String,
    pub event_content_sha256: String,
    /// Monotonic epoch for each bounded retention scan commit. Replicas use it
    /// to apply one small source/target delta without downloading the full
    /// cumulative archive.
    pub retention_generation: u64,
    /// Highest cursor made durable in an archive commit. WAL compaction uses
    /// this monotonic coverage even after an event expires.
    pub watermarks: ArchiveWatermarks,
    /// Highest cursor that is still present in each retained signal set.
    pub retained_watermarks: ArchiveWatermarks,
    /// Minimum retained event time. The lifecycle worker uses this fixed root
    /// field instead of scanning the full catalog every minute.
    pub oldest_event_time_unix_nano: Option<i64>,
    pub epochs: Vec<EpochMap>,
    pub catalog_uri: String,
    pub catalog_root: storage_segment::CatalogRoot,
    pub segment_count: u64,
    pub blob_count: u64,
    pub dedupe_receipt_count: u64,
    pub gc_plan_uri: Option<String>,
    pub gc_plan_root: Option<storage_segment::CatalogRoot>,
    pub gc_object_count: u64,
    /// One bounded retention generation can be applied by a caught-up voter
    /// without downloading the cumulative archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_delta: Option<ArchiveRetentionDelta>,
    /// Durable cursor for a bounded catalog scan. A non-empty value keeps the
    /// same cutoff until every segment entry has been visited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_scan: Option<ArchiveRetentionScan>,
    /// Runtime-only compatibility view. The remote V8 root never embeds the
    /// cumulative segment list.
    #[serde(skip, default)]
    pub segments: Vec<ArchiveSegment>,
    /// Runtime-only compatibility view. The remote V8 root never embeds the
    /// cumulative blob list.
    #[serde(skip, default)]
    pub blobs: Vec<ArchiveBlob>,
    /// Runtime-only compatibility view. The cleanup plan is a separate paged
    /// catalog and only a post-checkpoint leader may execute it.
    #[serde(skip, default)]
    pub gc_object_uris: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveRetentionDelta {
    pub source_manifest_uri: String,
    pub source_manifest_sha256: String,
    pub source_generation: u64,
    pub source_event_count: u64,
    pub source_event_content_sha256: String,
    pub cutoff_unix_nano: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveRetentionScan {
    pub cutoff_unix_nano: i64,
    pub after_catalog_key: String,
    pub oldest_retained_event_time_unix_nano: Option<i64>,
}

pub(in crate::archive) fn empty_dedupe_receipt() -> ArchiveDedupeReceipt {
    ArchiveDedupeReceipt {
        object_uri: String::new(),
        bytes: 0,
        sha256: String::new(),
        entry_count: 0,
        first_cursor: 0,
        last_cursor: 0,
        min_acknowledged_at_unix_nano: 0,
        max_acknowledged_at_unix_nano: 0,
    }
}
