//! Building a GC plan catalog from the spilled candidates and live objects.

use std::path::Path;

use anyhow::{Context, Result};

use crate::archive::application::archive_services::ArchiveServices;
use crate::archive::application::object_service::ObjectService;
use crate::archive::application::spill_service::SpillService as SpillCatalog;
use crate::archive::domain::control_paths::ARCHIVE_GC_PATH;
use crate::archive::domain::resource_ports::{CatalogBuild, CatalogEntry, CatalogReader};
use crate::shared_kernel::catalog_manifest::CatalogPageRef;
use crate::shared_kernel::sha256_hex::sha256_hex;

pub(in crate::archive) fn spill_pending_archive_gc(
    services: &ArchiveServices,
    root: &Path,
    target: &mut SpillCatalog,
) -> Result<()> {
    services.reconcile_staged_archive_gc(root)?;
    let path = root.join(ARCHIVE_GC_PATH);
    if !path.exists() {
        return Ok(());
    }
    let pending = services.read_archive_gc_pending(&path)?;
    let committed = services.read_commit_state(root)?;
    if committed.as_ref().is_none_or(|committed| {
        committed.manifest_uri != pending.replacement_manifest_uri
            || committed.manifest_sha256 != pending.replacement_manifest_sha256
    }) {
        services.remove_archive_gc_pending(&path)?;
        return Ok(());
    }
    let catalog = services.resources.catalog_for_uri(&pending.gc_plan_uri)?;
    let mut reader = match pending.cursor.as_deref() {
        Some(cursor) => catalog.reader_after(&pending.gc_plan_root, cursor)?,
        None => catalog.reader(&pending.gc_plan_root)?,
    };
    for entry in &mut reader {
        let entry = entry?;
        let uri = String::from_utf8(entry.value).context("decode pending archive GC URI")?;
        services
            .resources
            .gc_target(&uri)
            .with_context(|| format!("validate pending archive GC object URI {uri}"))?;
        target.insert_uri(&uri)?;
    }
    Ok(())
}

struct FilteredGcEntries<'a> {
    candidates: CatalogReader<storage_segment::SegmentError>,
    live: &'a SpillCatalog,
}

impl Iterator for FilteredGcEntries<'_> {
    type Item = storage_segment::Result<CatalogEntry>;

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
                    return Some(Ok(CatalogEntry {
                        key: format!("gc/{}", sha256_hex(uri.as_bytes())),
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
    services: &ArchiveServices,
    store: ObjectService,
    prefix: &str,
    candidates: &SpillCatalog,
    live: &SpillCatalog,
    spill_parent: &Path,
) -> Result<Option<CatalogBuild>> {
    let mut entries = FilteredGcEntries {
        candidates: candidates.reader()?,
        live,
    };
    let Some(first) = entries.next() else {
        return Ok(None);
    };
    let first = first?;
    let catalog = store.catalog(prefix)?;
    let mut written = services
        .resources
        .spill(spill_parent, "gc-catalog-written-")?;
    match catalog.build_sorted_observed(std::iter::once(Ok(first)).chain(entries), |reference| {
        record_catalog_page(&mut written, reference)
    }) {
        Ok(build) => Ok(Some(build)),
        Err(error) => {
            cleanup_observed_catalog_pages(&store, &written)?;
            Err(error.into())
        }
    }
}

pub(in crate::archive) fn record_catalog_page(
    ledger: &mut SpillCatalog,
    reference: &CatalogPageRef,
) -> storage_segment::Result<()> {
    ledger
        .upsert(
            format!("page/{}", sha256_hex(reference.key.as_bytes())),
            reference.key.as_bytes().to_vec(),
        )
        .map_err(|error| storage_segment::SegmentError::Serialization {
            message: format!("persist catalog cleanup ledger: {error:#}"),
        })
}

pub(in crate::archive) fn cleanup_observed_catalog_pages(
    store: &ObjectService,
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
