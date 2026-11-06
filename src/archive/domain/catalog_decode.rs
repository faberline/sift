//! Decode catalog values with the existing context and source errors.

use crate::archive::domain::archive_catalog_item::ArchiveCatalogItem;
use crate::archive::domain::resource_ports::CatalogEntry;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CatalogEntryError {
    #[error("decode archive segment catalog entry")]
    Segment(#[source] serde_json::Error),
    #[error("decode archive blob catalog entry")]
    Blob(#[source] serde_json::Error),
    #[error("decode archive dedupe receipt catalog entry")]
    Receipt(#[source] serde_json::Error),
    #[error("archive catalog contains an unknown key {0}")]
    UnknownKey(String),
}
pub(crate) fn decode_archive_catalog_entry(
    entry: CatalogEntry,
) -> Result<ArchiveCatalogItem, CatalogEntryError> {
    if entry.key.starts_with("segment/") {
        return serde_json::from_slice(&entry.value)
            .map(ArchiveCatalogItem::Segment)
            .map_err(CatalogEntryError::Segment);
    }
    if entry.key.starts_with("blob/") {
        return serde_json::from_slice(&entry.value)
            .map(ArchiveCatalogItem::Blob)
            .map_err(CatalogEntryError::Blob);
    }
    if entry.key.starts_with("receipt/") {
        return serde_json::from_slice(&entry.value)
            .map(ArchiveCatalogItem::Receipt)
            .map_err(CatalogEntryError::Receipt);
    }
    Err(CatalogEntryError::UnknownKey(entry.key))
}
