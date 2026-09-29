//! The pending and completed local blob GC records.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::archive::application::archive_receipts::ArchiveReceipt;
use crate::archive::infrastructure::archive_commit_state::read_commit_state;
use crate::archive::infrastructure::archive_control_paths::{
    LOCAL_BLOB_GC_COMPLETE_PATH, LOCAL_BLOB_GC_FORMAT_VERSION, LOCAL_BLOB_GC_PATH,
};
use crate::archive::infrastructure::local_blob_live_index::remove_local_blob_live_index;

#[cfg(any(not(unix), unix))]
use crate::archive::infrastructure::private_fs_mode::set_private_file;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::archive) struct LocalBlobGcPending {
    format_version: u16,
    replacement_manifest_uri: String,
    pub(in crate::archive) replacement_manifest_sha256: String,
    pub(in crate::archive) gc_plan_uri: String,
    pub(in crate::archive) gc_plan_root: storage_segment::CatalogRoot,
    pub(in crate::archive) plan_cursor: Option<String>,
    pub(in crate::archive) plan_exhausted: bool,
    pub(in crate::archive) candidates: Vec<String>,
    scan_start_cursor: u64,
    pub(in crate::archive) scanned_through_cursor: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LocalBlobGcComplete {
    format_version: u16,
    replacement_manifest_uri: String,
    pub(in crate::archive) replacement_manifest_sha256: String,
}

fn local_blob_gc_pending_for_receipt(receipt: &ArchiveReceipt) -> Option<LocalBlobGcPending> {
    let (Some(gc_plan_uri), Some(gc_plan_root)) = (
        receipt.manifest.gc_plan_uri.clone(),
        receipt.manifest.gc_plan_root.clone(),
    ) else {
        return None;
    };
    Some(LocalBlobGcPending {
        format_version: LOCAL_BLOB_GC_FORMAT_VERSION,
        replacement_manifest_uri: receipt.manifest_uri.clone(),
        replacement_manifest_sha256: receipt.manifest_sha256.clone(),
        gc_plan_uri,
        gc_plan_root,
        plan_cursor: None,
        plan_exhausted: false,
        candidates: Vec::new(),
        scan_start_cursor: receipt.manifest.raft_snapshot_index,
        scanned_through_cursor: receipt.manifest.raft_snapshot_index,
    })
}

pub(in crate::archive) fn ensure_local_blob_gc_pending(root: &Path) -> Result<()> {
    let path = root.join(LOCAL_BLOB_GC_PATH);
    if path.exists() {
        // Finish the prior committed plan before a newer manifest installs
        // its plan. Replacing this cursor can leak the older local blobs.
        read_local_blob_gc_pending(&path)?;
        return Ok(());
    }
    let Some(committed) = read_commit_state(root)? else {
        return Ok(());
    };
    let complete_path = root.join(LOCAL_BLOB_GC_COMPLETE_PATH);
    if complete_path.exists() {
        let complete = read_local_blob_gc_complete(&complete_path)?;
        if complete.replacement_manifest_uri == committed.manifest_uri
            && complete.replacement_manifest_sha256 == committed.manifest_sha256
        {
            remove_local_blob_live_index(root, &complete.replacement_manifest_sha256)?;
            return Ok(());
        }
    }
    let receipt = ArchiveReceipt {
        manifest_uri: committed.manifest_uri,
        manifest_sha256: committed.manifest_sha256,
        manifest: committed.manifest,
    };
    match local_blob_gc_pending_for_receipt(&receipt) {
        Some(pending) => persist_local_blob_gc_pending(&path, &pending),
        None => remove_local_blob_gc_pending(&path),
    }
}

pub(in crate::archive) fn complete_local_blob_gc_plan(
    root: &Path,
    pending_path: &Path,
    pending: &LocalBlobGcPending,
) -> Result<()> {
    let complete = LocalBlobGcComplete {
        format_version: LOCAL_BLOB_GC_FORMAT_VERSION,
        replacement_manifest_uri: pending.replacement_manifest_uri.clone(),
        replacement_manifest_sha256: pending.replacement_manifest_sha256.clone(),
    };
    persist_local_blob_gc_complete(&root.join(LOCAL_BLOB_GC_COMPLETE_PATH), &complete)?;
    remove_local_blob_gc_pending(pending_path)?;
    remove_local_blob_live_index(root, &pending.replacement_manifest_sha256)
}

pub(in crate::archive) fn read_local_blob_gc_pending(path: &Path) -> Result<LocalBlobGcPending> {
    let pending: LocalBlobGcPending =
        serde_json::from_slice(&std::fs::read(path)?).context("decode local blob GC progress")?;
    validate_local_blob_gc_pending(&pending)?;
    Ok(pending)
}

fn validate_local_blob_gc_pending(pending: &LocalBlobGcPending) -> Result<()> {
    if pending.format_version != LOCAL_BLOB_GC_FORMAT_VERSION
        || !pending.replacement_manifest_uri.starts_with("gs://")
        || pending.replacement_manifest_sha256.len() != 64
        || !pending
            .replacement_manifest_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !pending.gc_plan_uri.starts_with("gs://")
        || pending.scanned_through_cursor < pending.scan_start_cursor
        || pending.candidates.len() > 128
        || pending.candidates.iter().any(|hash| {
            hash.strip_prefix("sha256:").is_none_or(|digest| {
                digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        })
    {
        bail!("local blob GC progress has invalid fields");
    }
    Ok(())
}

fn read_local_blob_gc_complete(path: &Path) -> Result<LocalBlobGcComplete> {
    let complete: LocalBlobGcComplete = serde_json::from_slice(&std::fs::read(path)?)
        .context("decode completed local blob GC receipt")?;
    validate_local_blob_gc_complete(&complete)?;
    Ok(complete)
}

fn validate_local_blob_gc_complete(complete: &LocalBlobGcComplete) -> Result<()> {
    if complete.format_version != LOCAL_BLOB_GC_FORMAT_VERSION
        || !complete.replacement_manifest_uri.starts_with("gs://")
        || complete.replacement_manifest_sha256.len() != 64
        || !complete
            .replacement_manifest_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("completed local blob GC receipt has invalid fields");
    }
    Ok(())
}

fn persist_local_blob_gc_complete(path: &Path, complete: &LocalBlobGcComplete) -> Result<()> {
    validate_local_blob_gc_complete(complete)?;
    storage_durable::atomic_write(
        path,
        &serde_json::to_vec_pretty(complete)?,
        storage_durable::FsyncPolicy::Always,
    )?;
    set_private_file(path)
}

pub(in crate::archive) fn persist_local_blob_gc_pending(
    path: &Path,
    pending: &LocalBlobGcPending,
) -> Result<()> {
    validate_local_blob_gc_pending(pending)?;
    let bytes = serde_json::to_vec_pretty(pending)?;
    if bytes.len() >= 64 * 1024 {
        bail!("local blob GC progress exceeds 64 KiB");
    }
    storage_durable::atomic_write(path, &bytes, storage_durable::FsyncPolicy::Always)?;
    set_private_file(path)
}

fn remove_local_blob_gc_pending(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => storage_durable::sync_parent_dir(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
