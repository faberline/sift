//! Walk verified archive catalogs through their read ports.

use crate::archive::application::archive_services::ArchiveServices;
use crate::archive::domain::archive_catalog_item::ArchiveCatalogItem;
use crate::archive::domain::catalog_decode::decode_archive_catalog_entry;
use crate::shared_kernel::archive_manifest::{ArchiveBlob, ArchiveManifest, ArchiveSegment};
use anyhow::{bail, Context, Result};

pub(in crate::archive) fn load_archive_catalog(
    services: &ArchiveServices,
    manifest: &ArchiveManifest,
) -> Result<(Vec<ArchiveSegment>, Vec<ArchiveBlob>)> {
    let catalog = services.resources.catalog_for_uri(&manifest.catalog_uri)?;
    let mut segments = Vec::new();
    let mut blobs = Vec::new();
    let mut receipt_count = 0_u64;
    for entry in catalog.reader(&manifest.catalog_root)? {
        match decode_archive_catalog_entry(entry?)? {
            ArchiveCatalogItem::Segment(segment) => segments.push(segment),
            ArchiveCatalogItem::Blob(blob) => blobs.push(blob),
            ArchiveCatalogItem::Receipt(_) => {
                receipt_count = receipt_count.saturating_add(1);
            }
        }
    }
    segments.sort_by_key(|segment| segment.source.first_cursor);
    blobs.sort_by(|left, right| left.reference.hash.cmp(&right.reference.hash));
    if segments.len() as u64 != manifest.segment_count
        || blobs.len() as u64 != manifest.blob_count
        || receipt_count != manifest.dedupe_receipt_count
    {
        bail!("archive catalog counts disagree with its manifest root");
    }
    Ok((segments, blobs))
}

pub(in crate::archive) fn load_gc_catalog(
    services: &ArchiveServices,
    manifest: &ArchiveManifest,
) -> Result<Vec<String>> {
    let (Some(uri), Some(root)) = (&manifest.gc_plan_uri, &manifest.gc_plan_root) else {
        if manifest.gc_object_count != 0 {
            bail!("archive GC root is missing for a non-empty plan");
        }
        return Ok(Vec::new());
    };
    let catalog = services.resources.catalog_for_uri(uri)?;
    let mut uris = Vec::new();
    for entry in catalog.reader(root)? {
        let entry = entry?;
        if !entry.key.starts_with("gc/") {
            bail!("archive GC catalog contains an unknown key {}", entry.key);
        }
        uris.push(String::from_utf8(entry.value).context("decode archive GC object URI")?);
    }
    uris.sort();
    uris.dedup();
    if uris.len() as u64 != manifest.gc_object_count {
        bail!("archive GC catalog count disagrees with its manifest root");
    }
    Ok(uris)
}
