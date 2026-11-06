//! Applying a retention pass's catalog mutations and reading the retained
//! watermarks.

use anyhow::{bail, Result};

use crate::archive::application::spill_service::SpillService as SpillCatalog;
use crate::archive::domain::archive_catalog_item::ArchiveCatalogItem;
use crate::archive::domain::catalog_decode::decode_archive_catalog_entry;
use crate::archive::domain::gcs_uri::gcs_uri;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::SignalKind;

pub(super) fn apply_catalog_remove(
    catalog: &crate::archive::application::catalog_service::CatalogService,
    root: &mut crate::shared_kernel::catalog_manifest::CatalogRoot,
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
    catalog: &crate::archive::application::catalog_service::CatalogService,
    root: &mut crate::shared_kernel::catalog_manifest::CatalogRoot,
    entry: crate::archive::domain::resource_ports::CatalogEntry,
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
    catalog: &crate::archive::application::catalog_service::CatalogService,
    root: &crate::shared_kernel::catalog_manifest::CatalogRoot,
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
