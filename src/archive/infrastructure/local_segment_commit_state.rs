//! The local archive's commit record: which sealed segments it holds, and
//! retiring it.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::archive::infrastructure::archive_control_paths::{
    LOCAL_COMMIT_FORMAT_VERSION, LOCAL_COMMIT_PATH,
};
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::storage::SegmentManifest;
use crate::SignalKind;

#[cfg(any(not(unix), unix))]
use crate::archive::infrastructure::private_fs_mode::set_private_file;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::archive) struct LocalSegmentCommitState {
    pub(in crate::archive) format_version: u16,
    pub(in crate::archive) committed_at: String,
    pub(in crate::archive) snapshot_index: u64,
    pub(in crate::archive) watermarks: ArchiveWatermarks,
    pub(in crate::archive) segments: Vec<LocalCommittedSegment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::archive) struct LocalCommittedSegment {
    pub(in crate::archive) signal: SignalKind,
    pub(in crate::archive) manifest: SegmentManifest,
}

pub(in crate::archive) fn read_local_commit_state(
    root: &Path,
) -> Result<Option<LocalSegmentCommitState>> {
    let path = root.join(LOCAL_COMMIT_PATH);
    if !path.exists() {
        return Ok(None);
    }
    let state: LocalSegmentCommitState = serde_json::from_slice(
        &std::fs::read(&path)
            .with_context(|| format!("read local segment commit {}", path.display()))?,
    )
    .with_context(|| format!("decode local segment commit {}", path.display()))?;
    if state.format_version != LOCAL_COMMIT_FORMAT_VERSION {
        bail!(
            "unsupported local segment commit format {}; expected {}",
            state.format_version,
            LOCAL_COMMIT_FORMAT_VERSION
        );
    }
    let mut watermarks = ArchiveWatermarks::default();
    for segment in &state.segments {
        if segment.manifest.state != crate::storage::SegmentState::Sealed {
            bail!("local segment commit contains a non-sealed segment");
        }
        watermarks.include(segment.signal, segment.manifest.last_cursor);
        let manifest_path = root
            .join("segments")
            .join(signal_storage_dir(segment.signal))
            .join("manifests")
            .join(format!("{}.json", segment.manifest.segment_id));
        let current: SegmentManifest =
            serde_json::from_slice(&std::fs::read(&manifest_path).with_context(|| {
                format!(
                    "read committed local segment manifest {}",
                    manifest_path.display()
                )
            })?)
            .with_context(|| {
                format!(
                    "decode committed local segment manifest {}",
                    manifest_path.display()
                )
            })?;
        if current != segment.manifest {
            bail!(
                "committed local segment {} disagrees with its manifest",
                segment.manifest.segment_id
            );
        }
    }
    if watermarks != state.watermarks || watermarks.max_cursor() != state.snapshot_index {
        bail!("local segment commit watermarks do not match its manifest set");
    }
    set_private_file(&path)?;
    Ok(Some(state))
}

pub(in crate::archive) fn retire_local_commit(root: &Path) -> Result<()> {
    let path = root.join(LOCAL_COMMIT_PATH);
    match std::fs::remove_file(&path) {
        Ok(()) => storage_durable::sync_parent_dir(&path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| {
            format!(
                "retire local segment commit after remote archive {}",
                path.display()
            )
        }),
    }
}

fn signal_storage_dir(signal: SignalKind) -> &'static str {
    match signal {
        SignalKind::Log => "logs",
        SignalKind::Metric => "metrics",
        SignalKind::Span => "traces",
    }
}
