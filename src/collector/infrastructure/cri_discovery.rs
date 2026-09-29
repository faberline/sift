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
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            format!("{}:{}", metadata.dev(), metadata.ino())
        };
        #[cfg(not(unix))]
        bail!("CRI device/inode collection requires Unix");
        let relative_path = relative.to_string_lossy().to_string();
        files.push(DiscoveredFile {
            known_before: known.contains_key(&identity),
            identity,
            path,
            relative_path,
            len: metadata.len(),
            workload,
        });
    }
    Ok(files)
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
