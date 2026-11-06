//! Fetching the committed manifest's catalog root after verifying its hash.

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::archive::infrastructure::archive_catalog::catalog_for_uri;
use crate::archive::infrastructure::archive_commit_state::read_commit_state;
use crate::archive::infrastructure::manifest_validation::validate_archive_manifest;
use crate::shared_kernel::archive_manifest::ArchiveManifest;
use crate::shared_kernel::sha256_hex::sha256_hex;

pub(in crate::archive) fn fetch_verified_committed_root(
    root: &Path,
) -> Result<Option<ArchiveManifest>> {
    let Some(state) = read_commit_state(root)? else {
        return Ok(None);
    };
    let bytes = service_backup::fetch_backup_object(&state.manifest_uri)
        .context("fetch committed Sift archive manifest")?;
    if sha256_hex(&bytes) != state.manifest_sha256 {
        bail!("committed Sift archive manifest failed its SHA-256 check");
    }
    let manifest: ArchiveManifest =
        serde_json::from_slice(&bytes).context("decode committed Sift archive manifest")?;
    if manifest != state.manifest || manifest.watermarks != state.watermarks {
        bail!("remote Sift archive manifest disagrees with its local commit receipt");
    }
    validate_archive_manifest(&manifest)?;
    let catalog = catalog_for_uri(&manifest.catalog_uri)?;
    let mut reader = catalog.reader(&(&manifest.catalog_root).into())?;
    let _ = reader.next().transpose()?;
    Ok(Some(manifest))
}
