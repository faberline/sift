//! Finding the pod log files under the CRI root, without following symlinks,
//! and identifying each by device and inode.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::collector::domain::cri::{parse_workload_path, DiscoveredFile};
use crate::collector::infrastructure::cri_checkpoint::CriFileCheckpoint;

#[cfg(not(unix))]
use anyhow::bail;

pub(super) fn discover(
    root: &Path,
    known: &BTreeMap<String, CriFileCheckpoint>,
) -> Result<Vec<DiscoveredFile>> {
    let mut paths = Vec::new();
    visit_regular_files(root, root, 0, &mut paths)?;
    let mut files = Vec::new();
    for path in paths {
        let relative = path
            .strip_prefix(root)
            .context("CRI discovery escaped canonical root")?;
        let Some(workload) = parse_workload_path(relative)? else {
            continue;
        };
        let metadata = std::fs::symlink_metadata(&path)?;
        let physical_identity = file_identity(&metadata)?;
        let birth_nanos = file_birth(&metadata);
        let scoped_identity = format!("{physical_identity}:{}", workload.source_key());
        let legacy_identity = legacy_identity(&metadata)?;
        let identity = if known.contains_key(&scoped_identity) {
            scoped_identity
        } else if known.get(&legacy_identity).is_some_and(|file| {
            file.workload == workload && (file.birth_nanos == 0 || file.birth_nanos == birth_nanos)
        }) {
            // Keep old cursor and event identities for a known unchanged file.
            legacy_identity
        } else {
            scoped_identity
        };
        let relative_path = relative.to_string_lossy().to_string();
        files.push(DiscoveredFile {
            known_before: known.contains_key(&identity),
            identity,
            physical_identity,
            birth_nanos,
            path,
            relative_path,
            len: metadata.len(),
            workload,
        });
    }
    Ok(files)
}

pub(super) fn file_identity(metadata: &std::fs::Metadata) -> Result<String> {
    Ok(format!(
        "{}:{}",
        legacy_identity(metadata)?,
        file_birth(metadata)
    ))
}

fn file_birth(metadata: &std::fs::Metadata) -> u64 {
    metadata
        .created()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| duration.as_nanos().try_into().ok())
        .unwrap_or(0)
}

fn legacy_identity(metadata: &std::fs::Metadata) -> Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        bail!("CRI device/inode collection requires Unix")
    }
}

fn visit_regular_files(
    root: &Path,
    directory: &Path,
    depth: usize,
    output: &mut Vec<PathBuf>,
) -> Result<()> {
    if depth > 3 {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)
        .with_context(|| format!("read CRI directory {}", directory.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            visit_regular_files(root, &path, depth + 1, output)?;
        } else if file_type.is_file() && path.starts_with(root) {
            output.push(path);
        }
    }
    Ok(())
}
