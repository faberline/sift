//! Disk-spilled catalogs the archive builds GC plans and blob references in,
//! and cleaning up orphaned spills.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::archive::infrastructure::archive_integrity::sha256;
use crate::archive::infrastructure::ephemeral_file_object_store::EphemeralFileObjectStore;
use crate::ContentBlobRef;

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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(in crate::archive) struct SpillBlobReference {
    pub(in crate::archive) reference: ContentBlobRef,
    pub(in crate::archive) count: u64,
}

pub(in crate::archive) fn add_spill_blob_reference(
    target: &mut SpillCatalog,
    reference: &ContentBlobRef,
) -> Result<()> {
    let key = format!("blob/{}", reference.hash);
    let mut counted = target
        .lookup(&key)?
        .map(|bytes| {
            serde_json::from_slice::<SpillBlobReference>(&bytes)
                .context("decode archive spill blob reference")
        })
        .transpose()?
        .unwrap_or_else(|| SpillBlobReference {
            reference: reference.clone(),
            count: 0,
        });
    if counted.reference != *reference {
        bail!("one blob hash has conflicting content references");
    }
    counted.count = counted.count.saturating_add(1);
    target.upsert(key, serde_json::to_vec(&counted)?)
}

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

    pub(in crate::archive) fn add_u64(&mut self, key: String, amount: u64) -> Result<()> {
        let current = self
            .lookup(&key)?
            .map(|bytes| {
                bytes
                    .as_slice()
                    .try_into()
                    .map(u64::from_le_bytes)
                    .map_err(|_| anyhow::anyhow!("archive spill count is corrupt"))
            })
            .transpose()?
            .unwrap_or_default();
        self.upsert(key, current.saturating_add(amount).to_le_bytes().to_vec())
    }

    pub(in crate::archive) fn get_u64(&self, key: &str) -> Result<u64> {
        self.lookup(key)?
            .map(|bytes| {
                bytes
                    .as_slice()
                    .try_into()
                    .map(u64::from_le_bytes)
                    .map_err(|_| anyhow::anyhow!("archive spill count is corrupt"))
            })
            .transpose()
            .map(|value| value.unwrap_or_default())
    }

    pub(in crate::archive) fn insert_uri(&mut self, uri: &str) -> Result<()> {
        self.upsert(
            format!("uri/{}", sha256(uri.as_bytes())),
            uri.as_bytes().to_vec(),
        )
    }

    pub(in crate::archive) fn remove_uri(&mut self, uri: &str) -> Result<()> {
        self.remove(&format!("uri/{}", sha256(uri.as_bytes())))
    }

    pub(super) fn contains_uri(&self, uri: &str) -> Result<bool> {
        Ok(self
            .lookup(&format!("uri/{}", sha256(uri.as_bytes())))?
            .is_some())
    }

    pub(in crate::archive) fn reader(&self) -> Result<storage_segment::CatalogReader> {
        self.catalog.reader(&self.root).map_err(Into::into)
    }

    pub(in crate::archive) fn len(&self) -> u64 {
        self.root.entry_count
    }
}

impl crate::archive::domain::blob_hash_set::BlobHashSet for SpillCatalog {
    fn insert_hash(&mut self, hash: &str) -> Result<()> {
        self.upsert(format!("blob/{hash}"), Vec::new())
    }

    fn contains_hash(&self, hash: &str) -> Result<bool> {
        Ok(self.lookup(&format!("blob/{hash}"))?.is_some())
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
