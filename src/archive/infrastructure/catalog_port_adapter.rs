//! Resource adapters retain the original catalog, object store and transaction.

use crate::archive::domain::resource_ports::*;
use crate::shared_kernel::catalog_manifest::{CatalogPageRef, CatalogRoot};
use anyhow::Result;

impl From<storage_segment::CatalogEntry> for CatalogEntry {
    fn from(value: storage_segment::CatalogEntry) -> Self {
        Self {
            key: value.key().to_string(),
            value: value.value().to_vec(),
        }
    }
}
fn segment_entry(
    value: CatalogEntry,
) -> Result<storage_segment::CatalogEntry, storage_segment::SegmentError> {
    storage_segment::CatalogEntry::try_new(value.key, value.value)
}
fn mutation(value: storage_segment::CatalogMutation) -> CatalogMutation {
    CatalogMutation {
        root: value.root.into(),
        written_page_keys: value.written_page_keys,
        obsolete_page_keys: value.obsolete_page_keys,
    }
}
fn build(value: storage_segment::StreamingCatalogBuild) -> CatalogBuild {
    CatalogBuild {
        root: value.root.into(),
        written_page_count: value.written_page_count,
        peak_buffer_bytes: value.peak_buffer_bytes,
    }
}
pub(super) struct NativeCatalog(pub(super) storage_segment::PagedCatalog);
impl ArchiveCatalogPort for NativeCatalog {
    type Error = storage_segment::SegmentError;
    fn reader(&self, root: &CatalogRoot) -> Result<CatalogReader<Self::Error>, Self::Error> {
        Ok(Box::new(
            self.0
                .reader(&root.into())?
                .map(|entry| entry.map(Into::into)),
        ))
    }
    fn reader_after(
        &self,
        root: &CatalogRoot,
        after: &str,
    ) -> Result<CatalogReader<Self::Error>, Self::Error> {
        Ok(Box::new(
            self.0
                .reader_after(&root.into(), after)?
                .map(|entry| entry.map(Into::into)),
        ))
    }
    fn page_keys(&self, root: &CatalogRoot) -> Result<PageKeys<Self::Error>, Self::Error> {
        Ok(Box::new(self.0.page_keys(&root.into())?))
    }
    fn lookup(&self, root: &CatalogRoot, key: &str) -> Result<Option<CatalogEntry>, Self::Error> {
        self.0
            .lookup(&root.into(), key)
            .map(|entry| entry.map(Into::into))
    }
    fn last_with_prefix(
        &self,
        root: &CatalogRoot,
        prefix: &str,
    ) -> Result<Option<CatalogEntry>, Self::Error> {
        self.0
            .last_with_prefix(&root.into(), prefix)
            .map(|entry| entry.map(Into::into))
    }
    fn remove(&self, root: &CatalogRoot, key: &str) -> Result<CatalogMutation, Self::Error> {
        self.0.remove(&root.into(), key).map(mutation)
    }
    fn upsert(
        &self,
        root: &CatalogRoot,
        entry: CatalogEntry,
    ) -> Result<CatalogMutation, Self::Error> {
        self.0
            .upsert(&root.into(), segment_entry(entry)?)
            .map(mutation)
    }
    fn build_sorted_observed(
        &self,
        entries: &mut dyn Iterator<Item = Result<CatalogEntry, Self::Error>>,
        observed: &mut dyn FnMut(&CatalogPageRef) -> Result<(), Self::Error>,
    ) -> Result<CatalogBuild, Self::Error> {
        self.0
            .build_sorted_observed(
                entries.map(|entry| entry.and_then(segment_entry)),
                |reference| observed(&reference.clone().into()),
            )
            .map(build)
    }
}
