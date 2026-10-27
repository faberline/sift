//! Checkpoint positions and compaction outcomes shared by lifecycle services.

use crate::shared_kernel::event::SignalKind;
use crate::shared_kernel::segment_manifest::SegmentManifest;

pub(crate) type CapturedPrefix = (u64, u64, Vec<(SignalKind, SegmentManifest)>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SnapshotCompactionOutcome {
    pub(crate) snapshot_index: u64,
    pub(crate) installed: bool,
}
