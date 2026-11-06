//! catalog service over the original resource port.

use crate::archive::domain::resource_ports::*;
use crate::shared_kernel::catalog_manifest::CatalogPageRef;
use crate::shared_kernel::catalog_manifest::CatalogRoot;
use anyhow::Result;
use std::sync::Arc;

type CatalogError = storage_segment::SegmentError;
type CatalogPort = dyn ArchiveCatalogPort<Error = CatalogError>;

pub(crate) struct CatalogService {
    port: Arc<CatalogPort>,
}
impl CatalogService {
    pub(crate) fn new(port: Arc<CatalogPort>) -> Self {
        Self { port }
    }
    pub(crate) fn reader(
        &self,
        root: &CatalogRoot,
    ) -> Result<CatalogReader<CatalogError>, CatalogError> {
        self.port.reader(root)
    }
    pub(crate) fn reader_after(
        &self,
        root: &CatalogRoot,
        after: &str,
    ) -> Result<CatalogReader<CatalogError>, CatalogError> {
        self.port.reader_after(root, after)
    }
    pub(crate) fn page_keys(
        &self,
        root: &CatalogRoot,
    ) -> Result<PageKeys<CatalogError>, CatalogError> {
        self.port.page_keys(root)
    }
    pub(crate) fn lookup(
        &self,
        root: &CatalogRoot,
        key: &str,
    ) -> Result<Option<CatalogEntry>, CatalogError> {
        self.port.lookup(root, key)
    }
    pub(crate) fn last_with_prefix(
        &self,
        root: &CatalogRoot,
        prefix: &str,
    ) -> Result<Option<CatalogEntry>, CatalogError> {
        self.port.last_with_prefix(root, prefix)
    }
    pub(crate) fn remove(
        &self,
        root: &CatalogRoot,
        key: &str,
    ) -> Result<CatalogMutation, CatalogError> {
        self.port.remove(root, key)
    }
    pub(crate) fn upsert(
        &self,
        root: &CatalogRoot,
        entry: CatalogEntry,
    ) -> Result<CatalogMutation, CatalogError> {
        self.port.upsert(root, entry)
    }
    pub(crate) fn build_sorted_observed(
        &self,
        entries: impl IntoIterator<Item = Result<CatalogEntry, CatalogError>>,
        mut observed: impl FnMut(&CatalogPageRef) -> Result<(), CatalogError>,
    ) -> Result<CatalogBuild, CatalogError> {
        self.port
            .build_sorted_observed(&mut entries.into_iter(), &mut observed)
    }
}
