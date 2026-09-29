//! A segment manifest rewritten to the archive's portable, node-independent
//! path.

use std::path::PathBuf;

use crate::storage::SegmentManifest;
use crate::SignalKind;

pub(in crate::archive) fn portable_manifest(
    mut source: SegmentManifest,
    signal: SignalKind,
) -> SegmentManifest {
    source.local_path = PathBuf::from(format!("segments/{signal}/{}.framed", source.segment_id));
    source.object_uri = None;
    source
}
