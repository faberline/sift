//! Restricting archive control directories and files to the owner.

#[cfg(any(not(unix), unix))]
use std::path::Path;

#[cfg(any(not(unix), unix))]
use anyhow::Result;

#[cfg(unix)]
pub(in crate::archive) fn set_private_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
pub(in crate::archive) fn set_private_dir(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(in crate::archive) fn set_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
pub(in crate::archive) fn set_private_file(_path: &Path) -> Result<()> {
    Ok(())
}
