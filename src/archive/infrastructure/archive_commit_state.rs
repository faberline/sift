//! The remote archive's commit record, and adopting a verified archive receipt.

use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::archive::application::archive_receipts::ArchiveReceipt;
use crate::archive::domain::archive_manifest::{ArchiveManifest, ARCHIVE_FORMAT_VERSION};
use crate::archive::domain::archive_manifest_validator::validate_archive_manifest;
use crate::archive::infrastructure::archive_catalog::catalog_for_uri;
use crate::archive::infrastructure::archive_control_paths::{
    ARCHIVE_COMMIT_FORMAT_VERSION, ARCHIVE_COMMIT_PATH,
};
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;

#[cfg(any(not(unix), unix))]
use crate::archive::infrastructure::private_fs_mode::set_private_file;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::archive) struct ArchiveCommitState {
    pub(in crate::archive) format_version: u16,
    pub(in crate::archive) manifest_uri: String,
    pub(in crate::archive) manifest_sha256: String,
    pub(in crate::archive) committed_at: String,
    pub(in crate::archive) watermarks: ArchiveWatermarks,
    pub(in crate::archive) manifest: ArchiveManifest,
}

pub(in crate::archive) fn read_commit_state(root: &Path) -> Result<Option<ArchiveCommitState>> {
    let path = root.join(ARCHIVE_COMMIT_PATH);
    if !path.exists() {
        return Ok(None);
    }
    let state: ArchiveCommitState = serde_json::from_slice(
        &std::fs::read(&path)
            .with_context(|| format!("read archive commit receipt {}", path.display()))?,
    )
    .with_context(|| format!("decode archive commit receipt {}", path.display()))?;
    if state.format_version != ARCHIVE_COMMIT_FORMAT_VERSION {
        bail!(
            "unsupported archive commit format {}; expected {}",
            state.format_version,
            ARCHIVE_COMMIT_FORMAT_VERSION
        );
    }
    if state.manifest_uri.trim().is_empty()
        || state.manifest_sha256.len() != 64
        || !state
            .manifest_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("archive commit receipt has invalid manifest identity");
    }
    if state.manifest.format_version != ARCHIVE_FORMAT_VERSION
        || state.manifest.watermarks != state.watermarks
    {
        bail!("archive commit receipt has invalid embedded manifest");
    }
    set_private_file(&path)?;
    Ok(Some(state))
}

pub(in crate::archive) fn record_archive_commit(
    root: &Path,
    receipt: &ArchiveReceipt,
) -> Result<ArchiveWatermarks> {
    let watermarks = receipt.manifest.watermarks;
    let state = ArchiveCommitState {
        format_version: ARCHIVE_COMMIT_FORMAT_VERSION,
        manifest_uri: receipt.manifest_uri.clone(),
        manifest_sha256: receipt.manifest_sha256.clone(),
        committed_at: Utc::now().to_rfc3339(),
        watermarks,
        manifest: receipt.manifest.clone(),
    };
    let path = root.join(ARCHIVE_COMMIT_PATH);
    let bytes = serde_json::to_vec_pretty(&state)?;
    if bytes.len() >= 64 * 1024 {
        bail!("archive commit root exceeds 64 KiB");
    }
    storage_durable::atomic_write(&path, &bytes, storage_durable::FsyncPolicy::Always)?;
    set_private_file(&path)?;
    Ok(watermarks)
}

pub(crate) fn adopt_verified_archive_receipt(
    root: &Path,
    receipt: &ArchiveReceipt,
) -> Result<ArchiveWatermarks> {
    validate_archive_manifest(&receipt.manifest)?;
    let catalog = catalog_for_uri(&receipt.manifest.catalog_uri)?;
    let mut reader = catalog.reader(&receipt.manifest.catalog_root)?;
    let _ = reader.next().transpose()?;
    record_archive_commit(root, receipt)
}
