//! Metadata for the existing Sift snapshot stream.

use crate::shared_kernel::retention_fence::RetentionFenceV1;

pub const SNAPSHOT_CONTENT_TYPE: &str = "application/vnd.axiom.sift-snapshot";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SnapshotMetadata {
    pub applied_index: u64,
    pub last_cursor: u64,
    pub event_count: u64,
    /// `None` means the legacy full snapshot had no control-state field.
    /// `Some(None)` is an authoritative cleared fence.
    pub pending_retention: Option<Option<RetentionFenceV1>>,
}
