//! The restore's progress record and its staging directory.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::archive::infrastructure::archive_control_paths::{
    RESTORE_STAGE_DIR, RESTORE_STATE_FORMAT_VERSION, RESTORE_STATE_PATH,
};
use crate::archive::infrastructure::archive_gc_pending_store::remove_archive_gc_pending;

#[cfg(any(not(unix), unix))]
use crate::archive::infrastructure::private_fs_mode::set_private_file;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::archive) enum RestorePhase {
    Building,
    Ready,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::archive) struct RestoreState {
    pub(in crate::archive) format_version: u16,
    pub(in crate::archive) manifest_uri: String,
    pub(in crate::archive) phase: RestorePhase,
}

pub(in crate::archive) fn read_restore_state(path: &Path) -> Result<Option<RestoreState>> {
    if !path.exists() {
        return Ok(None);
    }
    let state: RestoreState = serde_json::from_slice(
        &std::fs::read(path)
            .with_context(|| format!("read cold restore state {}", path.display()))?,
    )
    .with_context(|| format!("decode cold restore state {}", path.display()))?;
    if state.format_version != RESTORE_STATE_FORMAT_VERSION
        || !state.manifest_uri.starts_with("gs://")
    {
        bail!("cold restore state has invalid identity fields");
    }
    set_private_file(path)?;
    Ok(Some(state))
}

pub(in crate::archive) fn persist_restore_state(path: &Path, state: &RestoreState) -> Result<()> {
    if state.format_version != RESTORE_STATE_FORMAT_VERSION
        || !state.manifest_uri.starts_with("gs://")
    {
        bail!("cold restore state has invalid identity fields");
    }
    let bytes = serde_json::to_vec_pretty(state)?;
    storage_durable::atomic_write(path, &bytes, storage_durable::FsyncPolicy::Always)?;
    set_private_file(path)
}

pub(in crate::archive) fn validate_building_restore_target(target: &Path) -> Result<()> {
    for entry in std::fs::read_dir(target)
        .with_context(|| format!("read interrupted cold restore target {}", target.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        if name == "lost+found" || name == RESTORE_STATE_PATH || name == RESTORE_STAGE_DIR {
            continue;
        }
        bail!(
            "interrupted cold restore target contains an unexpected entry: {}",
            entry.path().display()
        );
    }
    Ok(())
}

pub(in crate::archive) fn publish_restore_stage(target: &Path) -> Result<()> {
    let state_path = target.join(RESTORE_STATE_PATH);
    let state = read_restore_state(&state_path)?
        .context("cold restore publication requires a durable restore state")?;
    if state.phase != RestorePhase::Ready {
        bail!("cold restore stage is not ready for publication");
    }
    let stage = target.join(RESTORE_STAGE_DIR);
    if !stage.exists() {
        let layout: crate::storage::LayoutManifest = serde_json::from_slice(
            &std::fs::read(target.join("layout.json"))
                .context("resume cold restore publication without a layout")?,
        )
        .context("decode resumed cold restore layout")?;
        if layout.restored_from.as_deref() != Some(state.manifest_uri.as_str()) {
            bail!("resumed cold restore layout has the wrong source manifest");
        }
        remove_archive_gc_pending(&state_path)?;
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(&stage)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("cold restore stage is not a real directory");
    }
    let mut entries = std::fs::read_dir(&stage)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|path| path.file_name() == Some(std::ffi::OsStr::new("layout.json")));
    for source in entries {
        let name = source
            .file_name()
            .context("cold restore stage entry has no file name")?;
        let destination = target.join(name);
        if destination.exists() {
            bail!(
                "cold restore publication found an existing destination: {}",
                destination.display()
            );
        }
        std::fs::rename(&source, &destination).with_context(|| {
            format!(
                "publish cold restore entry {} to {}",
                source.display(),
                destination.display()
            )
        })?;
        storage_durable::sync_parent_dir(&destination)?;
    }
    std::fs::remove_dir(&stage)
        .with_context(|| format!("remove published cold restore stage {}", stage.display()))?;
    storage_durable::sync_parent_dir(&stage)?;
    remove_archive_gc_pending(&state_path)
}

pub(in crate::archive) fn remove_restore_stage(stage: &Path) -> Result<()> {
    match std::fs::symlink_metadata(stage) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            bail!("cold restore stage is not a real directory")
        }
        Ok(_) => std::fs::remove_dir_all(stage).with_context(|| {
            format!("remove interrupted cold restore stage {}", stage.display())
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    storage_durable::sync_parent_dir(stage)
}

pub(in crate::archive) fn require_empty_restore_target(target: &Path) -> Result<()> {
    if target.exists() {
        let entries = std::fs::read_dir(target)
            .with_context(|| format!("read cold restore target {}", target.display()))?;
        let mut has_owned_entry = false;
        for entry in entries {
            if entry?.file_name() != "lost+found" {
                has_owned_entry = true;
                break;
            }
        }
        if has_owned_entry {
            bail!(
                "cold restore requires an empty data directory: {}",
                target.display()
            );
        }
    }
    Ok(())
}
