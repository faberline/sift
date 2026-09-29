//! What archiving, eviction, expiration and replay report back, and the archive
//! status.

use crate::archive::domain::archive_manifest::ArchiveManifest;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;

#[derive(Clone, Debug)]
pub struct ArchiveReceipt {
    pub manifest_uri: String,
    pub manifest_sha256: String,
    pub manifest: ArchiveManifest,
}

#[derive(Clone, Debug)]
pub struct LocalArchiveReceipt {
    pub committed_at: String,
    pub snapshot_index: u64,
    pub watermarks: ArchiveWatermarks,
    pub event_count: u64,
    pub segment_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HotEvictionReceipt {
    pub evicted_segments: usize,
    pub evicted_events: u64,
    pub evicted_through_cursor: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpirationReceipt {
    pub manifest_uri: String,
    pub retained_events: u64,
    pub retained_segments: usize,
    pub expired_events: u64,
    pub replaced_segments: usize,
    pub removed_segments: usize,
}

#[derive(Clone, Debug)]
pub struct ArchiveCommitStatus {
    pub manifest_uri: String,
    pub manifest_sha256: String,
    pub committed_at: String,
    pub snapshot_index: u64,
    pub watermarks: ArchiveWatermarks,
    pub retention_generation: u64,
    pub retention_scan_pending: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LocalArchiveStatus {
    pub snapshot_index: u64,
    pub watermarks: ArchiveWatermarks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RemoteRetainedState {
    pub watermarks: ArchiveWatermarks,
    pub event_count: u64,
    pub snapshot_index: u64,
    pub retention_generation: u64,
    pub retention_scan_pending: bool,
    pub event_content_sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveReplay {
    pub watermark: u64,
    pub scanned: u64,
    pub replayed: u64,
}
