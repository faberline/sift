//! The committed and local archive's watermarks, retained state and status.

use crate::archive::application::archive_services::ArchiveServices;

use std::path::Path;

use anyhow::Result;

use crate::archive::domain::event_set_digest::decode_event_content_digest;
use crate::shared_kernel::archive_receipts::{
    ArchiveCommitStatus, LocalArchiveStatus, RemoteRetainedState,
};
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;

pub(crate) fn committed_watermarks(
    services: &ArchiveServices,
    root: &Path,
) -> Result<ArchiveWatermarks> {
    let remote = committed_status(services, root)?
        .map(|status| status.watermarks)
        .unwrap_or_default();
    Ok(remote.merge(local_committed_watermarks(services, root)?))
}

pub(crate) fn remote_retained_state(
    services: &ArchiveServices,
    root: &Path,
) -> Result<Option<RemoteRetainedState>> {
    services
        .read_commit_state(root)?
        .map(|state| {
            Ok(RemoteRetainedState {
                watermarks: state.watermarks,
                event_count: state.manifest.event_count,
                snapshot_index: state.manifest.raft_snapshot_index,
                retention_generation: state.manifest.retention_generation,
                retention_scan_pending: state.manifest.retention_scan.is_some(),
                event_content_sha256: decode_event_content_digest(
                    &state.manifest.event_content_sha256,
                )?,
            })
        })
        .transpose()
}

pub(crate) fn local_committed_watermarks(
    services: &ArchiveServices,
    root: &Path,
) -> Result<ArchiveWatermarks> {
    Ok(services
        .read_local_commit_state(root)?
        .map(|state| state.watermarks)
        .unwrap_or_default())
}

pub(crate) fn local_committed_status(
    services: &ArchiveServices,
    root: &Path,
) -> Result<Option<LocalArchiveStatus>> {
    Ok(services
        .read_local_commit_state(root)?
        .map(|state| LocalArchiveStatus {
            snapshot_index: state.snapshot_index,
            watermarks: state.watermarks,
        }))
}

pub(crate) fn committed_status(
    services: &ArchiveServices,
    root: &Path,
) -> Result<Option<ArchiveCommitStatus>> {
    Ok(services
        .read_commit_state(root)?
        .map(|state| ArchiveCommitStatus {
            manifest_uri: state.manifest_uri,
            manifest_sha256: state.manifest_sha256,
            committed_at: state.committed_at,
            snapshot_index: state.manifest.raft_snapshot_index,
            watermarks: state.watermarks,
            retention_generation: state.manifest.retention_generation,
            retention_scan_pending: state.manifest.retention_scan.is_some(),
        }))
}

/// Verify that the last locally committed archive manifest is still readable.
/// A cold query uses this check before it claims a complete answer. The local
/// commit receipt is not enough because GCS access can be removed after the
/// commit was written.
pub(crate) fn verify_committed_manifest_available(
    services: &ArchiveServices,
    root: &Path,
) -> Result<bool> {
    Ok(services.fetch_verified_committed_root(root)?.is_some())
}
