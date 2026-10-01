//! Disk-spilled catalogs the archive builds GC plans and blob references in,
//! and cleaning up orphaned spills.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};

use crate::archive::infrastructure::ephemeral_file_object_store::EphemeralFileObjectStore;

const ARCHIVE_SPILL_PREFIXES: [&str; 8] = [
    "retention-obsolete-",
    "retention-live-",
    "archive-updates-",
    "archive-blob-counts-",
    "archive-obsolete-",
    "archive-live-",
    "restore-hot-blobs-",
    "restore-blob-counts-",
];

/// A temporary, disk-backed ordered map used by archive maintenance.
///
/// Each mutation keeps only the paged-catalog search path in memory. Replaced
/// copy-on-write pages are deleted immediately. The whole temporary directory
/// disappears when the operation ends.
pub(in crate::archive) struct SpillCatalog {
    store: Arc<EphemeralFileObjectStore>,
    catalog: storage_segment::PagedCatalog,
    pub(in crate::archive) root: storage_segment::CatalogRoot,
    _directory: tempfile::TempDir,
}

impl SpillCatalog {
    pub(in crate::archive) fn new(parent: &Path, name: &str) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix(name)
            .tempdir_in(parent)
            .with_context(|| format!("create archive spill directory in {}", parent.display()))?;
        let store = Arc::new(EphemeralFileObjectStore::new(directory.path()));
        let catalog = storage_segment::PagedCatalog::new(store.clone(), "catalog")?;
        let root = catalog.build_sorted(std::iter::empty())?.root;
        Ok(Self {
            store,
            catalog,
            root,
            _directory: directory,
        })
    }

    pub(in crate::archive) fn upsert(&mut self, key: String, value: Vec<u8>) -> Result<()> {
        let mutation = self
            .catalog
            .upsert(&self.root, storage_segment::CatalogEntry { key, value })?;
        let written = mutation
            .written_page_keys
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        for obsolete in &mutation.obsolete_page_keys {
            if !written.contains(obsolete.as_str()) {
                storage_object::ObjectStore::delete(self.store.as_ref(), obsolete)?;
            }
        }
        self.root = mutation.root;
        Ok(())
    }

    pub(in crate::archive) fn lookup(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .catalog
            .lookup(&self.root, key)?
            .map(|entry| entry.value))
    }

    pub(in crate::archive) fn remove(&mut self, key: &str) -> Result<()> {
        let mutation = self.catalog.remove(&self.root, key)?;
        let written = mutation
            .written_page_keys
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        for obsolete in &mutation.obsolete_page_keys {
            if !written.contains(obsolete.as_str()) {
                storage_object::ObjectStore::delete(self.store.as_ref(), obsolete)?;
            }
        }
        self.root = mutation.root;
        Ok(())
    }

    pub(in crate::archive) fn reader(&self) -> Result<storage_segment::CatalogReader> {
        self.catalog.reader(&self.root).map_err(Into::into)
    }
}

pub(crate) fn cleanup_orphan_spills(root: &Path) -> Result<usize> {
    let tmp = root.join("tmp");
    if !tmp.exists() {
        return Ok(0);
    }
    storage_durable::reject_symlink(&tmp)?;
    let mut removed = 0_usize;
    for entry in std::fs::read_dir(&tmp)
        .with_context(|| format!("list Sift temporary directory {}", tmp.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !ARCHIVE_SPILL_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_symlink() || !file_type.is_dir() {
            bail!("archive spill path is not a real directory");
        }
        std::fs::remove_dir_all(entry.path())
            .with_context(|| format!("remove orphan archive spill {}", entry.path().display()))?;
        removed = removed.saturating_add(1);
    }
    if removed > 0 {
        storage_durable::sync_parent_dir(tmp.join("spill-cleanup"))?;
    }
    Ok(removed)
}
