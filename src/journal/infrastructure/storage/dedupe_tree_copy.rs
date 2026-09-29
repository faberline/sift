//! Copying and removing dedupe directory trees.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::journal::infrastructure::storage::dedupe_generation_store::META_FILE;
use crate::journal::infrastructure::storage::dedupe_shard_file::{
    set_private_dir, set_private_file,
};

pub(super) fn copy_dedupe_tree(source: &Path, target: &Path) -> Result<()> {
    let source_meta = fs::symlink_metadata(source)?;
    if source_meta.file_type().is_symlink() || !source_meta.is_dir() {
        bail!("source dedupe index is not a real directory");
    }
    fs::create_dir(target)?;
    set_private_dir(target)?;
    let mut generations = Vec::new();
    let mut meta = None::<PathBuf>;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            bail!("source dedupe index contains a symlink");
        }
        if entry.file_name() == META_FILE && file_type.is_file() {
            meta = Some(entry.path());
        } else if file_type.is_dir() {
            generations.push(entry.path());
        } else {
            bail!("source dedupe index contains an unexpected entry");
        }
    }
    generations.sort();
    for generation in generations {
        let name = generation
            .file_name()
            .context("source dedupe generation has no name")?;
        let destination = target.join(name);
        fs::create_dir(&destination)?;
        set_private_dir(&destination)?;
        let mut shards = fs::read_dir(&generation)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        shards.sort();
        for shard in shards {
            let metadata = fs::symlink_metadata(&shard)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                bail!("source dedupe generation contains a special file");
            }
            let target_file = destination.join(
                shard
                    .file_name()
                    .context("source dedupe shard has no file name")?,
            );
            fs::copy(&shard, &target_file)?;
            set_private_file(&target_file)?;
            File::open(&target_file)?.sync_all()?;
            storage_durable::sync_parent_dir(&target_file)?;
        }
    }
    let meta = meta.context("source dedupe index has no durable metadata")?;
    let target_meta = target.join(META_FILE);
    fs::copy(meta, &target_meta)?;
    set_private_file(&target_meta)?;
    File::open(&target_meta)?.sync_all()?;
    storage_durable::sync_parent_dir(&target_meta)?;
    storage_durable::sync_parent_dir(target)
}

pub(super) fn remove_real_dir_if_exists(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            bail!(
                "dedupe replacement path {} is not a real directory",
                path.display()
            )
        }
        Ok(_) => fs::remove_dir_all(path)
            .with_context(|| format!("remove dedupe replacement path {}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    storage_durable::sync_parent_dir(path)
}

pub(super) fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            bail!("dedupe replacement marker is not a regular file")
        }
        Ok(_) => fs::remove_file(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    storage_durable::sync_parent_dir(path)
}
