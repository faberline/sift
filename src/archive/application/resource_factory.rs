//! Create operation-local resources at the original I/O points.

use crate::archive::application::catalog_service::CatalogService;
use crate::archive::application::object_service::ObjectService;
use crate::archive::application::signal_service::SignalService;
use crate::archive::application::spill_service::SpillService;
use crate::archive::domain::resource_ports::{ArchiveResources, CatalogReader};
use crate::SignalKind;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

type ResourcePort = dyn ArchiveResources<
    Error = anyhow::Error,
    CatalogError = storage_segment::SegmentError,
    ObjectError = storage_object::ObjectStoreError,
>;
pub(crate) struct ResourceFactory {
    port: Arc<ResourcePort>,
}
impl ResourceFactory {
    pub(crate) fn fetch_object(&self, uri: &str) -> Result<Vec<u8>> {
        self.port.fetch_object(uri)
    }
    pub(crate) fn gc_target(
        &self,
        uri: &str,
    ) -> Result<Box<dyn crate::archive::domain::resource_ports::GcObjectPort<Error = anyhow::Error>>>
    {
        self.port.gc_target(uri)
    }

    pub(crate) fn parse_destination(&self, uri: &str) -> Result<(String, String)> {
        self.port.parse_destination(uri)
    }
    pub(crate) fn archive_dedupe_receipts(
        &self,
        transaction: &mut crate::archive::application::object_service::TransactionService,
        bucket: &str,
        prefix: &str,
        events: &[crate::shared_kernel::stored_event::StoredEvent],
    ) -> Result<crate::shared_kernel::archive_manifest::ArchiveDedupeReceipt> {
        self.port
            .archive_dedupe_receipts(transaction.port_mut(), bucket, prefix, events)
    }

    pub(crate) fn new(port: Arc<ResourcePort>) -> Self {
        Self { port }
    }
    pub(crate) fn catalog_for_uri(&self, uri: &str) -> Result<CatalogService> {
        self.port.catalog_for_uri(uri).map(CatalogService::new)
    }
    pub(crate) fn gcs_store(
        &self,
        bucket: &str,
    ) -> Result<ObjectService, storage_object::ObjectStoreError> {
        self.port.gcs_store(bucket).map(ObjectService::new)
    }
    pub(crate) fn spill(&self, parent: &Path, name: &str) -> Result<SpillService> {
        self.port.spill(parent, name).map(SpillService::new)
    }
    pub(crate) fn signal_stream(
        &self,
        root: &Path,
        catalog: CatalogReader<storage_segment::SegmentError>,
        signal: SignalKind,
        after: u64,
    ) -> SignalService {
        SignalService::new(self.port.signal_stream(root, catalog, signal, after))
    }
}
