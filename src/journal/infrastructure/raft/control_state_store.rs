//! Persisting the state machine's control record with private file modes.

use std::path::Path;

use anyhow::{Context, Result};

use crate::journal::domain::control_state::ControlState;

#[cfg(unix)]
use std::fs;

pub(super) const CONTROL_STATE_FILE: &str = "sift-control-state.json";

pub(super) fn persist_control(path: &Path, control: &ControlState) -> Result<()> {
    storage_durable::atomic_write(
        path,
        &serde_json::to_vec_pretty(control)?,
        storage_durable::FsyncPolicy::Always,
    )
    .with_context(|| format!("atomically persist Sift control state {}", path.display()))?;
    set_file_mode(path)
}

#[cfg(unix)]
pub(super) fn set_directory_mode(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("set private control directory mode on {}", path.display()))
}

#[cfg(not(unix))]
pub(super) fn set_directory_mode(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_file_mode(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("set private control file mode on {}", path.display()))
}

#[cfg(not(unix))]
fn set_file_mode(_path: &Path) -> Result<()> {
    Ok(())
}
