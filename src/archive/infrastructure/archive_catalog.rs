//! Opening and decoding the archive and GC plan catalogs.

use std::sync::Arc;

use anyhow::Result;

use crate::archive::domain::archive_catalog_item::ArchiveCatalogItem;
use crate::archive::domain::gcs_uri::split_gcs_uri;

pub(in crate::archive) fn catalog_for_uri(uri: &str) -> Result<storage_segment::PagedCatalog> {
    let (bucket, prefix) = split_gcs_uri(uri)?;
    storage_segment::PagedCatalog::new(
        Arc::new(storage_object::GcsObjectStore::new(&bucket, "")?),
        prefix,
    )
    .map_err(Into::into)
}

pub(in crate::archive) fn decode_archive_catalog_entry(
    entry: storage_segment::CatalogEntry,
) -> Result<ArchiveCatalogItem> {
    Ok(crate::archive::domain::catalog_decode::decode_archive_catalog_entry(entry.into())?)
}
