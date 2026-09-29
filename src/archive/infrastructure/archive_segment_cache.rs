//! The bounded local cache of fetched archive segments.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::archive::domain::archive_manifest::ArchiveSegment;
use crate::archive::infrastructure::archive_integrity::{verify_bytes, verify_file};

#[cfg(any(not(unix), unix))]
use crate::archive::infrastructure::private_fs_mode::set_private_file;

const ARCHIVE_CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub(in crate::archive) fn cached_segment_bytes(
    root: &Path,
    segment: &ArchiveSegment,
) -> Result<Vec<u8>> {
    let cache_path = cached_segment_path(root, segment)?;
    std::fs::read(&cache_path)
        .with_context(|| format!("read archive cache {}", cache_path.display()))
}

pub(super) fn cached_segment_path(root: &Path, segment: &ArchiveSegment) -> Result<PathBuf> {
    if segment.parquet_sha256.len() != 64
        || !segment
            .parquet_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("archive segment has an invalid Parquet SHA-256 value");
    }
    let cache_path = root
        .join("archive-cache")
        .join(format!("{}.parquet", segment.parquet_sha256));
    if cache_path.exists() {
        if verify_file(
            &cache_path,
            &segment.parquet_sha256,
            segment.parquet_bytes,
            "cached Parquet segment",
        )
        .is_ok()
        {
            set_private_file(&cache_path)?;
            prune_archive_cache(root, &cache_path)?;
            return Ok(cache_path);
        }
    }

    let bytes = service_backup::fetch_backup_object(&segment.object_uri)
        .with_context(|| format!("fetch archive segment {}", segment.object_uri))?;
    verify_bytes(
        &segment.parquet_sha256,
        segment.parquet_bytes,
        &bytes,
        "Parquet segment",
    )?;
    storage_durable::atomic_write(&cache_path, &bytes, storage_durable::FsyncPolicy::Always)?;
    set_private_file(&cache_path)?;
    prune_archive_cache(root, &cache_path)?;
    Ok(cache_path)
}

fn prune_archive_cache(root: &Path, keep: &Path) -> Result<()> {
    let cache_root = root.join("archive-cache");
    let mut total = 0_u64;
    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(&cache_root)
        .with_context(|| format!("read archive cache {}", cache_root.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("parquet") {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file() {
            bail!(
                "archive cache entry {} is not a regular file",
                path.display()
            );
        }
        total = total.saturating_add(metadata.len());
        candidates.push((metadata.modified().ok(), path, metadata.len()));
    }
    candidates.sort_by_key(|(modified, path, _)| (*modified, path.clone()));
    for (_, path, bytes) in candidates {
        if total <= ARCHIVE_CACHE_MAX_BYTES {
            break;
        }
        if path == keep {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {
                total = total.saturating_sub(bytes);
                storage_durable::sync_parent_dir(&path)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                total = total.saturating_sub(bytes);
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
