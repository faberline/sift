//! Restoring a volume from a committed GCS archive, and bootstrapping a fresh
//! one once.

use crate::archive::application::archive_services::ArchiveServices;

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::archive::application::restore_into_empty::restore_gcs_into_empty;
use crate::archive::domain::control_paths::{
    RESTORE_STAGE_DIR, RESTORE_STATE_FORMAT_VERSION, RESTORE_STATE_PATH,
};
use crate::archive::domain::control_values::{RestorePhase, RestoreState};
use crate::shared_kernel::archive_manifest::ArchiveManifest;

/// Restore through a resumable staging directory inside the target volume.
/// The live data-root names are published only after every remote object and
/// digest has been verified. A crash during download restarts the private
/// stage. A crash during publication resumes the remaining renames.
pub(crate) fn restore_gcs(
    services: &ArchiveServices,
    manifest_uri: &str,
    target: impl AsRef<Path>,
) -> Result<ArchiveManifest> {
    let target = target.as_ref();
    std::fs::create_dir_all(target)
        .with_context(|| format!("create cold restore target {}", target.display()))?;
    services.set_private_dir(target)?;
    let state_path = target.join(RESTORE_STATE_PATH);
    let stage = target.join(RESTORE_STAGE_DIR);
    let existing = services.read_restore_state(&state_path)?;
    match existing {
        Some(state) => {
            if state.manifest_uri != manifest_uri {
                bail!(
                    "cold restore staging belongs to a different manifest: {}",
                    state.manifest_uri
                );
            }
            if state.phase == RestorePhase::Ready {
                services.publish_restore_stage(target)?;
                return services
                    .fetch_verified_committed_root(target)?
                    .context("published cold restore has no committed archive receipt");
            }
            services.validate_building_restore_target(target)?;
            services.remove_restore_stage(&stage)?;
        }
        None => {
            services.require_empty_restore_target(target)?;
            services.persist_restore_state(
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
    services.set_private_dir(&stage)?;
    storage_durable::sync_parent_dir(&stage)?;

    let manifest = restore_gcs_into_empty(services, manifest_uri, &stage)?;
    services.persist_restore_state(
        &state_path,
        &RestoreState {
            format_version: RESTORE_STATE_FORMAT_VERSION,
            manifest_uri: manifest_uri.to_string(),
            phase: RestorePhase::Ready,
        },
    )?;
    services.publish_restore_stage(target)?;
    Ok(manifest)
}

/// Bootstrap a fresh volume once. A normal pod restart sees the same
/// `restored_from` value and reuses the completed restore without rewriting it.
pub(crate) fn bootstrap_gcs_if_needed(
    services: &ArchiveServices,
    manifest_uri: &str,
    target: impl AsRef<Path>,
) -> Result<Option<ArchiveManifest>> {
    let target = target.as_ref();
    if target.join(RESTORE_STATE_PATH).exists() {
        return restore_gcs(services, manifest_uri, target).map(Some);
    }
    let layout_path = target.join("layout.json");
    if layout_path.exists() {
        let layout: crate::node::infrastructure::data_layout::LayoutManifest =
            serde_json::from_slice(
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
    restore_gcs(services, manifest_uri, target).map(Some)
}
