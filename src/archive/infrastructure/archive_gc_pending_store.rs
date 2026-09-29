//! The pending archive GC record: staging, promoting, installing and
//! withholding a plan.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::archive::application::archive_receipts::ArchiveReceipt;
use crate::archive::infrastructure::archive_commit_state::read_commit_state;
use crate::archive::infrastructure::archive_control_paths::{
    ARCHIVE_GC_FORMAT_VERSION, ARCHIVE_GC_PATH, ARCHIVE_GC_STAGED_PATH,
};
use crate::archive::infrastructure::archive_upload_intent::reconcile_completed_archive_upload_intent;

#[cfg(any(not(unix), unix))]
use crate::archive::infrastructure::private_fs_mode::set_private_file;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::archive) struct ArchiveGcPending {
    pub(in crate::archive) format_version: u16,
    pub(in crate::archive) replacement_manifest_uri: String,
    pub(in crate::archive) replacement_manifest_sha256: String,
    pub(in crate::archive) gc_plan_uri: String,
    pub(in crate::archive) gc_plan_root: storage_segment::CatalogRoot,
    pub(in crate::archive) cursor: Option<String>,
}

fn archive_gc_pending_for_receipt(receipt: &ArchiveReceipt) -> Option<ArchiveGcPending> {
    let (Some(gc_plan_uri), Some(gc_plan_root)) = (
        receipt.manifest.gc_plan_uri.clone(),
        receipt.manifest.gc_plan_root.clone(),
    ) else {
        return None;
    };
    Some(ArchiveGcPending {
        format_version: ARCHIVE_GC_FORMAT_VERSION,
        replacement_manifest_uri: receipt.manifest_uri.clone(),
        replacement_manifest_sha256: receipt.manifest_sha256.clone(),
        gc_plan_uri,
        gc_plan_root,
        cursor: None,
    })
}

fn write_archive_gc_pending(root: &Path, receipt: &ArchiveReceipt) -> Result<()> {
    let path = root.join(ARCHIVE_GC_PATH);
    match archive_gc_pending_for_receipt(receipt) {
        Some(pending) => persist_archive_gc_pending(&path, &pending),
        None => remove_archive_gc_pending(&path),
    }
}

pub(in crate::archive) fn stage_archive_gc_pending(
    root: &Path,
    receipt: &ArchiveReceipt,
) -> Result<()> {
    let path = root.join(ARCHIVE_GC_STAGED_PATH);
    match archive_gc_pending_for_receipt(receipt) {
        Some(pending) => persist_archive_gc_pending(&path, &pending),
        None => remove_archive_gc_pending(&path),
    }
}

pub(in crate::archive) fn promote_archive_gc_pending(
    root: &Path,
    receipt: &ArchiveReceipt,
) -> Result<()> {
    let staged_path = root.join(ARCHIVE_GC_STAGED_PATH);
    let pending_path = root.join(ARCHIVE_GC_PATH);
    let Some(expected) = archive_gc_pending_for_receipt(receipt) else {
        remove_archive_gc_pending(&staged_path)?;
        return remove_archive_gc_pending(&pending_path);
    };
    let staged = read_archive_gc_pending(&staged_path)
        .context("promote committed archive GC staged intent")?;
    if staged != expected {
        bail!("archive GC staged intent does not match the committed manifest");
    }
    std::fs::rename(&staged_path, &pending_path)
        .context("atomically promote committed archive GC intent")?;
    set_private_file(&pending_path)?;
    storage_durable::sync_parent_dir(&pending_path)
}

pub(crate) fn reconcile_staged_archive_gc(root: &Path) -> Result<()> {
    reconcile_completed_archive_upload_intent(root)?;
    let staged_path = root.join(ARCHIVE_GC_STAGED_PATH);
    if !staged_path.exists() {
        return Ok(());
    }
    let staged = read_archive_gc_pending(&staged_path)?;
    let Some(committed) = read_commit_state(root)? else {
        return remove_archive_gc_pending(&staged_path);
    };
    let receipt = ArchiveReceipt {
        manifest_uri: committed.manifest_uri,
        manifest_sha256: committed.manifest_sha256,
        manifest: committed.manifest,
    };
    if archive_gc_pending_for_receipt(&receipt).as_ref() == Some(&staged) {
        promote_archive_gc_pending(root, &receipt)
    } else {
        remove_archive_gc_pending(&staged_path)
    }
}

pub(crate) fn install_archive_gc_plan(root: &Path, receipt: &ArchiveReceipt) -> Result<()> {
    write_archive_gc_pending(root, receipt)
}

/// Remove local archive-deletion authority when a quorum-only checkpoint is
/// installed. The immutable objects remain valid recovery sources until a
/// later all-voter checkpoint installs the exact plan again.
pub(crate) fn withhold_archive_gc_plan(root: &Path) -> Result<()> {
    let path = root.join(ARCHIVE_GC_PATH);
    match std::fs::remove_file(&path) {
        Ok(()) => storage_durable::sync_parent_dir(&path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn archive_gc_pending(root: &Path) -> bool {
    root.join(ARCHIVE_GC_PATH).exists()
}

pub(in crate::archive) fn read_archive_gc_pending(path: &Path) -> Result<ArchiveGcPending> {
    let pending: ArchiveGcPending = serde_json::from_slice(
        &std::fs::read(path)
            .with_context(|| format!("read archive GC receipt {}", path.display()))?,
    )
    .with_context(|| format!("decode archive GC receipt {}", path.display()))?;
    validate_archive_gc_pending(&pending)?;
    Ok(pending)
}

fn validate_archive_gc_pending(pending: &ArchiveGcPending) -> Result<()> {
    if pending.format_version != ARCHIVE_GC_FORMAT_VERSION
        || !pending.replacement_manifest_uri.starts_with("gs://")
        || pending.replacement_manifest_sha256.len() != 64
        || !pending
            .replacement_manifest_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !pending.gc_plan_uri.starts_with("gs://")
    {
        bail!("archive GC pending root has invalid identity fields");
    }
    Ok(())
}

pub(in crate::archive) fn persist_archive_gc_pending(
    path: &Path,
    pending: &ArchiveGcPending,
) -> Result<()> {
    validate_archive_gc_pending(pending)?;
    let bytes = serde_json::to_vec_pretty(pending)?;
    if bytes.len() >= 64 * 1024 {
        bail!("archive GC pending root exceeds 64 KiB");
    }
    storage_durable::atomic_write(path, &bytes, storage_durable::FsyncPolicy::Always)?;
    set_private_file(path)
}

pub(in crate::archive) fn remove_archive_gc_pending(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => storage_durable::sync_parent_dir(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
