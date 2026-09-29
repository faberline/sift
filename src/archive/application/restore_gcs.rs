//! Restoring a volume from a committed GCS archive, and bootstrapping a fresh
//! one once.

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::archive::application::restore_into_empty::restore_gcs_into_empty;
use crate::archive::domain::archive_manifest::ArchiveManifest;
use crate::archive::infrastructure::archive_control_paths::{
    RESTORE_STAGE_DIR, RESTORE_STATE_FORMAT_VERSION, RESTORE_STATE_PATH,
};
use crate::archive::infrastructure::restore_state::{
    persist_restore_state, publish_restore_stage, read_restore_state, remove_restore_stage,
    require_empty_restore_target, validate_building_restore_target, RestorePhase, RestoreState,
};
use crate::archive::infrastructure::verified_manifest_fetcher::fetch_verified_committed_root;

#[cfg(any(not(unix), unix))]
use crate::archive::infrastructure::private_fs_mode::set_private_dir;

/// Restore through a resumable staging directory inside the target volume.
/// The live data-root names are published only after every remote object and
/// digest has been verified. A crash during download restarts the private
/// stage. A crash during publication resumes the remaining renames.
pub fn restore_gcs(manifest_uri: &str, target: impl AsRef<Path>) -> Result<ArchiveManifest> {
    let target = target.as_ref();
    std::fs::create_dir_all(target)
        .with_context(|| format!("create cold restore target {}", target.display()))?;
    set_private_dir(target)?;
    let state_path = target.join(RESTORE_STATE_PATH);
    let stage = target.join(RESTORE_STAGE_DIR);
    let existing = read_restore_state(&state_path)?;
    match existing {
        Some(state) => {
            if state.manifest_uri != manifest_uri {
                bail!(
                    "cold restore staging belongs to a different manifest: {}",
                    state.manifest_uri
                );
            }
            if state.phase == RestorePhase::Ready {
                publish_restore_stage(target)?;
                return fetch_verified_committed_root(target)?
                    .context("published cold restore has no committed archive receipt");
            }
            validate_building_restore_target(target)?;
            remove_restore_stage(&stage)?;
        }
        None => {
            require_empty_restore_target(target)?;
            persist_restore_state(
                &state_path,
                &RestoreState {
                    format_version: RESTORE_STATE_FORMAT_VERSION,
                    manifest_uri: manifest_uri.to_string(),
                    phase: RestorePhase::Building,
                },
            )?;
        }
    }
    std::fs::create_dir(&stage)
        .with_context(|| format!("create cold restore stage {}", stage.display()))?;
    set_private_dir(&stage)?;
    storage_durable::sync_parent_dir(&stage)?;

    let manifest = restore_gcs_into_empty(manifest_uri, &stage)?;
    persist_restore_state(
        &state_path,
        &RestoreState {
            format_version: RESTORE_STATE_FORMAT_VERSION,
            manifest_uri: manifest_uri.to_string(),
            phase: RestorePhase::Ready,
        },
    )?;
    publish_restore_stage(target)?;
    Ok(manifest)
}

/// Bootstrap a fresh volume once. A normal pod restart sees the same
/// `restored_from` value and reuses the completed restore without rewriting it.
pub fn bootstrap_gcs_if_needed(
    manifest_uri: &str,
    target: impl AsRef<Path>,
) -> Result<Option<ArchiveManifest>> {
    let target = target.as_ref();
    if target.join(RESTORE_STATE_PATH).exists() {
        return restore_gcs(manifest_uri, target).map(Some);
    }
    let layout_path = target.join("layout.json");
    if layout_path.exists() {
        let layout: crate::storage::LayoutManifest = serde_json::from_slice(
            &std::fs::read(&layout_path)
                .with_context(|| format!("read bootstrap layout {}", layout_path.display()))?,
        )
        .with_context(|| format!("decode bootstrap layout {}", layout_path.display()))?;
        if layout.restored_from.as_deref() == Some(manifest_uri) {
            return Ok(None);
        }
        bail!(
            "bootstrap archive cannot overwrite an existing Sift data directory: {}",
            target.display()
        );
    }
    restore_gcs(manifest_uri, target).map(Some)
}
