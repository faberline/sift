//! Recover the manifest-last commit after an interrupted upload.
use crate::archive::application::manifest_validation::validate_archive_manifest;
use crate::archive::domain::control_values::ArchiveUploadIntent;
use crate::archive::domain::gcs_uri::split_gcs_uri;
use crate::shared_kernel::archive_manifest::ArchiveManifest;
use crate::shared_kernel::archive_receipts::ArchiveReceipt;
use crate::shared_kernel::sha256_hex::sha256_hex;
use anyhow::{bail, Context, Result};

pub(in crate::archive) fn recover_uploaded_archive(
    store: &crate::archive::application::object_service::ObjectService,
    intent: &ArchiveUploadIntent,
) -> Result<Option<ArchiveReceipt>> {
    let (_, manifest_key) = split_gcs_uri(&intent.manifest_uri)?;
    let object = match store.get(&manifest_key) {
        Ok(object) => object,
        Err(storage_object::ObjectStoreError::NotFound { .. }) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let manifest_sha256 = sha256_hex(&object.bytes);
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
