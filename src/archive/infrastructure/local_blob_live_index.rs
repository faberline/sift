//! The on-disk index of local blobs a GC pass has seen referenced.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

fn local_blob_live_index_root(root: &Path, replacement_sha256: &str) -> Result<PathBuf> {
    if replacement_sha256.len() != 64
        || !replacement_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("local blob GC replacement digest is invalid");
    }
    Ok(root
        .join("indexes")
        .join("local-blob-gc")
        .join(replacement_sha256))
}

fn local_blob_marker_path(root: &Path, replacement_sha256: &str, hash: &str) -> Result<PathBuf> {
    let digest = hash
        .strip_prefix("sha256:")
        .context("local blob hash must use sha256")?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("local blob hash is invalid");
    }
    Ok(local_blob_live_index_root(root, replacement_sha256)?
        .join(&digest[..2])
        .join(&digest[2..]))
}

pub(in crate::archive) fn mark_local_blob_live(
    root: &Path,
    replacement_sha256: &str,
    hash: &str,
) -> Result<()> {
    let path = local_blob_marker_path(root, replacement_sha256, hash)?;
    if path.exists() {
        let metadata = std::fs::symlink_metadata(&path)
            .with_context(|| format!("inspect local blob live marker {}", path.display()))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            bail!("local blob live marker is not a regular file");
        }
        return Ok(());
    }
    let parent = path
        .parent()
        .context("local blob live marker has no parent")?;
    let index_root = local_blob_live_index_root(root, replacement_sha256)?;
    std::fs::create_dir_all(&index_root)
        .with_context(|| format!("create local blob live index {}", index_root.display()))?;
    storage_durable::reject_symlink(&index_root)?;
    storage_durable::set_private_directory_mode(&index_root)?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create local blob live shard {}", parent.display()))?;
    storage_durable::reject_symlink(parent)?;
    storage_durable::set_private_directory_mode(parent)?;
    storage_durable::atomic_write(&path, b"live\n", storage_durable::FsyncPolicy::Always)?;
    storage_durable::set_private_file_mode(&path)
}

pub(in crate::archive) fn local_blob_is_live(
    root: &Path,
    replacement_sha256: &str,
    hash: &str,
) -> Result<bool> {
    let path = local_blob_marker_path(root, replacement_sha256, hash)?;
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => bail!("local blob live marker is not a regular file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("inspect local blob live marker {}", path.display()))
        }
    }
}

pub(super) fn remove_local_blob_live_index(root: &Path, replacement_sha256: &str) -> Result<()> {
    let path = local_blob_live_index_root(root, replacement_sha256)?;
    storage_durable::reject_symlink(&path)?;
    match std::fs::remove_dir_all(&path) {
        Ok(()) => storage_durable::sync_parent_dir(&path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("remove completed local blob live index {}", path.display())),
    }
}
