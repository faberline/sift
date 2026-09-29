//! The upload intent recorded before an archive upload starts, and its
//! recovery.

use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::archive::application::archive_receipts::ArchiveReceipt;
use crate::archive::domain::archive_manifest::ArchiveManifest;
use crate::archive::domain::archive_manifest_validator::validate_archive_manifest;
use crate::archive::infrastructure::archive_commit_state::{read_commit_state, ArchiveCommitState};
use crate::archive::infrastructure::archive_control_paths::{
    ARCHIVE_UPLOAD_INTENT_FORMAT_VERSION, ARCHIVE_UPLOAD_INTENT_PATH,
};
use crate::archive::infrastructure::archive_integrity::sha256;
use crate::archive::infrastructure::gcs_uri::split_gcs_uri;

#[cfg(any(not(unix), unix))]
use crate::archive::infrastructure::private_fs_mode::set_private_file;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::archive) struct ArchiveUploadIntent {
    pub(in crate::archive) format_version: u16,
    destination_uri: String,
    pub(in crate::archive) archive_prefix: String,
    pub(in crate::archive) manifest_uri: String,
    pub(in crate::archive) generated_at: String,
    pub(in crate::archive) captured_cursor: u64,
    pub(in crate::archive) source_cluster_id: String,
    source_manifest_uri: Option<String>,
    source_manifest_sha256: Option<String>,
}

pub(in crate::archive) fn prepare_archive_upload_intent(
    root: &Path,
    destination_uri: &str,
    destination_prefix: &str,
    cluster_id: &str,
    captured_cursor: u64,
    previous: Option<&ArchiveCommitState>,
) -> Result<ArchiveUploadIntent> {
    let path = root.join(ARCHIVE_UPLOAD_INTENT_PATH);
    if path.exists() {
        let intent = read_archive_upload_intent(&path)?;
        if previous.is_some_and(|state| {
            state.manifest_uri == intent.manifest_uri
                && state.manifest.raft_snapshot_index == intent.captured_cursor
        }) {
            remove_archive_upload_intent(&path)?;
        } else {
            let previous_uri = previous.map(|state| state.manifest_uri.as_str());
            let previous_hash = previous.map(|state| state.manifest_sha256.as_str());
            if intent.destination_uri != destination_uri
                || intent.source_cluster_id != cluster_id
                || intent.source_manifest_uri.as_deref() != previous_uri
                || intent.source_manifest_sha256.as_deref() != previous_hash
                || captured_cursor < intent.captured_cursor
            {
                bail!("unfinished archive upload intent does not match the captured prefix");
            }
            return Ok(intent);
        }
    }

    let source_token = previous
        .map(|state| state.manifest_sha256[..16].to_string())
        .unwrap_or_else(|| "root".to_string());
    let archive_id = format!(
        "{}-c{captured_cursor:020}-s{source_token}",
        &sha256(cluster_id.as_bytes())[..16]
    );
    let archive_prefix = format!("{destination_prefix}/archives/{archive_id}");
    let intent = ArchiveUploadIntent {
        format_version: ARCHIVE_UPLOAD_INTENT_FORMAT_VERSION,
        destination_uri: destination_uri.to_string(),
        manifest_uri: format!("{destination_uri}/archives/{archive_id}/manifest.json"),
        archive_prefix,
        generated_at: Utc::now().to_rfc3339(),
        captured_cursor,
        source_cluster_id: cluster_id.to_string(),
        source_manifest_uri: previous.map(|state| state.manifest_uri.clone()),
        source_manifest_sha256: previous.map(|state| state.manifest_sha256.clone()),
    };
    persist_archive_upload_intent(&path, &intent)?;
    Ok(intent)
}

pub(in crate::archive) fn recover_uploaded_archive(
    store: &dyn storage_object::ObjectStore,
    intent: &ArchiveUploadIntent,
) -> Result<Option<ArchiveReceipt>> {
    let (_, manifest_key) = split_gcs_uri(&intent.manifest_uri)?;
    let object = match store.get(&manifest_key) {
        Ok(object) => object,
        Err(storage_object::ObjectStoreError::NotFound { .. }) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let manifest_sha256 = sha256(&object.bytes);
    let manifest: ArchiveManifest = serde_json::from_slice(&object.bytes)
        .context("decode manifest recovered from archive upload intent")?;
    validate_archive_manifest(&manifest)?;
    if manifest.source_cluster_id != intent.source_cluster_id
        || manifest.raft_snapshot_index != intent.captured_cursor
        || manifest.generated_at != intent.generated_at
    {
        bail!("uploaded archive manifest does not match its durable intent");
    }
    Ok(Some(ArchiveReceipt {
        manifest_uri: intent.manifest_uri.clone(),
        manifest_sha256,
        manifest,
    }))
}

fn read_archive_upload_intent(path: &Path) -> Result<ArchiveUploadIntent> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("archive upload intent is not a regular file");
    }
    let intent: ArchiveUploadIntent =
        serde_json::from_slice(&std::fs::read(path)?).context("decode archive upload intent")?;
    validate_archive_upload_intent(&intent)?;
    Ok(intent)
}

fn validate_archive_upload_intent(intent: &ArchiveUploadIntent) -> Result<()> {
    if intent.format_version != ARCHIVE_UPLOAD_INTENT_FORMAT_VERSION
        || !intent.destination_uri.starts_with("gs://")
        || !intent.manifest_uri.starts_with(&intent.destination_uri)
        || intent.archive_prefix.is_empty()
        || intent.generated_at.is_empty()
        || intent.captured_cursor == 0
        || intent.source_cluster_id.is_empty()
        || intent.source_manifest_uri.is_some() != intent.source_manifest_sha256.is_some()
        || intent.source_manifest_sha256.as_ref().is_some_and(|hash| {
            hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        bail!("archive upload intent has invalid identity fields");
    }
    Ok(())
}

fn persist_archive_upload_intent(path: &Path, intent: &ArchiveUploadIntent) -> Result<()> {
    validate_archive_upload_intent(intent)?;
    storage_durable::atomic_write(
        path,
        &serde_json::to_vec_pretty(intent)?,
        storage_durable::FsyncPolicy::Always,
    )?;
    set_private_file(path)
}

fn remove_archive_upload_intent(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => storage_durable::sync_parent_dir(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(in crate::archive) fn clear_archive_upload_intent(
    root: &Path,
    receipt: &ArchiveReceipt,
) -> Result<()> {
    let path = root.join(ARCHIVE_UPLOAD_INTENT_PATH);
    if !path.exists() {
        return Ok(());
    }
    let intent = read_archive_upload_intent(&path)?;
    if intent.manifest_uri != receipt.manifest_uri
        || intent.captured_cursor != receipt.manifest.raft_snapshot_index
    {
        bail!("archive commit does not match its durable upload intent");
    }
    remove_archive_upload_intent(&path)
}

pub(super) fn reconcile_completed_archive_upload_intent(root: &Path) -> Result<()> {
    let path = root.join(ARCHIVE_UPLOAD_INTENT_PATH);
    if !path.exists() {
        return Ok(());
    }
    let intent = read_archive_upload_intent(&path)?;
    let Some(committed) = read_commit_state(root)? else {
        return Ok(());
    };
    if committed.manifest_uri == intent.manifest_uri
        && committed.manifest.raft_snapshot_index == intent.captured_cursor
    {
        remove_archive_upload_intent(&path)?;
    } else if intent.source_manifest_uri.as_deref() != Some(committed.manifest_uri.as_str())
        || intent.source_manifest_sha256.as_deref() != Some(committed.manifest_sha256.as_str())
    {
        // The local commit advanced from a different path, such as retention,
        // before this manifest-last upload recorded its receipt. The upload is
        // now an unreachable orphan. Keeping its intent would block every
        // later archive because its source identity can never match again.
        remove_archive_upload_intent(&path)?;
    }
    Ok(())
}
