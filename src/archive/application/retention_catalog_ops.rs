//! Applying a retention pass's catalog mutations and reading the retained
//! watermarks.

use anyhow::{bail, Result};

use crate::archive::domain::archive_catalog_item::ArchiveCatalogItem;
use crate::archive::infrastructure::archive_catalog::decode_archive_catalog_entry;
use crate::archive::infrastructure::gcs_uri::gcs_uri;
use crate::archive::infrastructure::spill_catalog::SpillCatalog;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::SignalKind;

pub(super) fn apply_catalog_remove(
    catalog: &storage_segment::PagedCatalog,
    root: &mut storage_segment::CatalogRoot,
    key: &str,
    bucket: &str,
    gc_candidates: &mut SpillCatalog,
    live_objects: &mut SpillCatalog,
) -> Result<()> {
    let mutation = catalog.remove(root, key)?;
    for obsolete in &mutation.obsolete_page_keys {
        let uri = gcs_uri(bucket, obsolete);
        gc_candidates.insert_uri(&uri)?;
        live_objects.remove_uri(&uri)?;
    }
    for written in &mutation.written_page_keys {
        live_objects.insert_uri(&gcs_uri(bucket, written))?;
    }
    *root = mutation.root;
    Ok(())
}

pub(super) fn apply_catalog_upsert(
    catalog: &storage_segment::PagedCatalog,
    root: &mut storage_segment::CatalogRoot,
    entry: storage_segment::CatalogEntry,
    bucket: &str,
    gc_candidates: &mut SpillCatalog,
    live_objects: &mut SpillCatalog,
) -> Result<()> {
    let mutation = catalog.upsert(root, entry)?;
    for obsolete in &mutation.obsolete_page_keys {
        let uri = gcs_uri(bucket, obsolete);
        gc_candidates.insert_uri(&uri)?;
        live_objects.remove_uri(&uri)?;
    }
    for written in &mutation.written_page_keys {
        live_objects.insert_uri(&gcs_uri(bucket, written))?;
    }
    *root = mutation.root;
    Ok(())
}

pub(super) fn retained_watermarks_from_catalog(
    catalog: &storage_segment::PagedCatalog,
    root: &storage_segment::CatalogRoot,
) -> Result<ArchiveWatermarks> {
    let mut watermarks = ArchiveWatermarks::default();
    for signal in SignalKind::ALL {
        let prefix = format!("segment/{signal}/");
        let Some(entry) = catalog.last_with_prefix(root, &prefix)? else {
            continue;
        };
        let ArchiveCatalogItem::Segment(segment) = decode_archive_catalog_entry(entry)? else {
            bail!("archive signal tail resolved to a blob");
        };
        if segment.signal != signal {
            bail!("archive signal tail resolved to another signal");
        }
        watermarks.include(signal, segment.source.last_cursor);
    }
    Ok(watermarks)
}
