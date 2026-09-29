//! Building a GC plan catalog from the spilled candidates and live objects.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::archive::infrastructure::archive_catalog::catalog_for_uri;
use crate::archive::infrastructure::archive_commit_state::read_commit_state;
use crate::archive::infrastructure::archive_control_paths::ARCHIVE_GC_PATH;
use crate::archive::infrastructure::archive_gc_pending_store::{
    read_archive_gc_pending, reconcile_staged_archive_gc, remove_archive_gc_pending,
};
use crate::archive::infrastructure::archive_integrity::sha256;
use crate::archive::infrastructure::spill_catalog::SpillCatalog;

pub(in crate::archive) fn spill_pending_archive_gc(
    root: &Path,
    target: &mut SpillCatalog,
) -> Result<()> {
    reconcile_staged_archive_gc(root)?;
    let path = root.join(ARCHIVE_GC_PATH);
    if !path.exists() {
        return Ok(());
    }
    let pending = read_archive_gc_pending(&path)?;
    let committed = read_commit_state(root)?;
    if committed.as_ref().is_none_or(|committed| {
        committed.manifest_uri != pending.replacement_manifest_uri
            || committed.manifest_sha256 != pending.replacement_manifest_sha256
    }) {
        remove_archive_gc_pending(&path)?;
        return Ok(());
    }
    let catalog = catalog_for_uri(&pending.gc_plan_uri)?;
    let mut reader = match pending.cursor.as_deref() {
        Some(cursor) => catalog.reader_after(&pending.gc_plan_root, cursor)?,
        None => catalog.reader(&pending.gc_plan_root)?,
    };
    for entry in &mut reader {
        let entry = entry?;
        let uri = String::from_utf8(entry.value).context("decode pending archive GC URI")?;
        service_backup::GcsSink::from_exact_uri(&uri)
            .with_context(|| format!("validate pending archive GC object URI {uri}"))?;
        target.insert_uri(&uri)?;
    }
    Ok(())
}

struct FilteredGcEntries<'a> {
    candidates: storage_segment::CatalogReader,
    live: &'a SpillCatalog,
}

impl Iterator for FilteredGcEntries<'_> {
    type Item = storage_segment::Result<storage_segment::CatalogEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = self.candidates.next()?;
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => return Some(Err(error)),
            };
            let uri = match String::from_utf8(entry.value) {
                Ok(uri) => uri,
                Err(error) => {
                    return Some(Err(storage_segment::SegmentError::Serialization {
                        message: error.to_string(),
                    }))
                }
            };
            match self.live.contains_uri(&uri) {
                Ok(true) => continue,
                Ok(false) => {
                    return Some(Ok(storage_segment::CatalogEntry {
                        key: format!("gc/{}", sha256(uri.as_bytes())),
                        value: uri.into_bytes(),
                    }))
                }
                Err(error) => {
                    return Some(Err(storage_segment::SegmentError::Serialization {
                        message: error.to_string(),
                    }))
                }
            }
        }
    }
}

pub(in crate::archive) fn build_gc_catalog_from_spills(
    store: Arc<dyn storage_object::ObjectStore>,
    prefix: &str,
    candidates: &SpillCatalog,
    live: &SpillCatalog,
    spill_parent: &Path,
) -> Result<Option<storage_segment::StreamingCatalogBuild>> {
    let mut entries = FilteredGcEntries {
        candidates: candidates.reader()?,
        live,
    };
    let Some(first) = entries.next() else {
        return Ok(None);
    };
    let first = first?;
    let catalog = storage_segment::PagedCatalog::new(store.clone(), prefix)?;
    let mut written = SpillCatalog::new(spill_parent, "gc-catalog-written-")?;
    match catalog.build_sorted_observed(std::iter::once(Ok(first)).chain(entries), |reference| {
        record_catalog_page(&mut written, reference)
    }) {
        Ok(build) => Ok(Some(build)),
        Err(error) => {
            cleanup_observed_catalog_pages(store.as_ref(), &written)?;
            Err(error.into())
        }
    }
}

pub(in crate::archive) fn record_catalog_page(
    ledger: &mut SpillCatalog,
    reference: &storage_segment::CatalogPageRef,
) -> storage_segment::Result<()> {
    ledger
        .upsert(
            format!("page/{}", sha256(reference.key.as_bytes())),
            reference.key.as_bytes().to_vec(),
        )
        .map_err(|error| storage_segment::SegmentError::Serialization {
            message: format!("persist catalog cleanup ledger: {error:#}"),
        })
}

pub(in crate::archive) fn cleanup_observed_catalog_pages(
    store: &dyn storage_object::ObjectStore,
    ledger: &SpillCatalog,
) -> Result<()> {
    for entry in ledger.reader()? {
        let key = String::from_utf8(entry?.value).context("decode catalog cleanup ledger key")?;
        store
            .delete(&key)
            .with_context(|| format!("remove page from aborted catalog build {key}"))?;
    }
    Ok(())
}
