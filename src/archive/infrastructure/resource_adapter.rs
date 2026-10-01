//! Resource adapters retain the original catalog, object store and transaction.

use crate::archive::domain::resource_ports::*;
use crate::archive::infrastructure::archive_signal_stream::ArchiveSignalStream;
use crate::archive::infrastructure::spill_catalog::SpillCatalog;
use crate::shared_kernel::catalog_manifest::CatalogRoot;
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::archive::infrastructure::catalog_port_adapter::NativeCatalog;

struct NativeSpill(SpillCatalog);
impl ArchiveSpillPort for NativeSpill {
    type Error = anyhow::Error;
    type CatalogError = storage_segment::SegmentError;
    fn root(&self) -> CatalogRoot {
        self.0.root.clone().into()
    }
    fn upsert(&mut self, key: String, value: Vec<u8>) -> Result<()> {
        self.0.upsert(key, value)
    }
    fn lookup(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.0.lookup(key)
    }
    fn remove(&mut self, key: &str) -> Result<()> {
        self.0.remove(key)
    }
    fn reader(&self) -> Result<CatalogReader<Self::CatalogError>> {
        Ok(Box::new(
            self.0.reader()?.map(|entry| entry.map(Into::into)),
        ))
    }
}
struct NativeTransaction(storage_segment::ArchiveTransaction);
impl ArchiveTransactionPort for NativeTransaction {
    type Error = storage_segment::SegmentError;
    fn put(&mut self, object: ArchiveObject) -> Result<(), Self::Error> {
        self.0
            .put(storage_segment::ArchiveObject::new(
                object.key,
                object.bytes,
                object.content_type,
            ))
            .map(|_| ())
    }
    fn commit(self: Box<Self>, manifest: ArchiveObject) -> Result<CommittedObject, Self::Error> {
        let committed = self.0.commit(storage_segment::ArchiveObject::new(
            manifest.key,
            manifest.bytes,
            manifest.content_type,
        ))?;
        Ok(CommittedObject {
            key: committed.manifest.key,
            size: committed.manifest.size,
            sha256: committed.manifest.sha256,
        })
    }
}
struct NativeObjectStore(Arc<dyn storage_object::ObjectStore>);

pub(crate) fn object_store_port(
    store: Arc<dyn storage_object::ObjectStore>,
) -> Arc<
    dyn ArchiveObjectPort<
        Error = storage_object::ObjectStoreError,
        CatalogError = storage_segment::SegmentError,
    >,
> {
    Arc::new(NativeObjectStore(store))
}

impl ArchiveObjectPort for NativeObjectStore {
    type Error = storage_object::ObjectStoreError;
    type CatalogError = storage_segment::SegmentError;
    fn get(&self, key: &str) -> Result<Vec<u8>, Self::Error> {
        self.0.get(key).map(|object| object.bytes)
    }
    fn delete(&self, key: &str) -> Result<(), Self::Error> {
        self.0.delete(key)
    }
    fn catalog(
        &self,
        prefix: &str,
    ) -> Result<Arc<dyn ArchiveCatalogPort<Error = Self::CatalogError>>, Self::CatalogError> {
        Ok(Arc::new(NativeCatalog(storage_segment::PagedCatalog::new(
            self.0.clone(),
            prefix,
        )?)))
    }
    fn transaction(&self) -> Box<dyn ArchiveTransactionPort<Error = Self::CatalogError>> {
        Box::new(NativeTransaction(
            storage_segment::ArchiveCoordinator::new(self.0.clone()).begin(),
        ))
    }
}
struct NativeSignal(ArchiveSignalStream);
impl ArchiveSignalPort for NativeSignal {
    type Error = anyhow::Error;
    fn peek_cursor(&mut self) -> Result<Option<u64>> {
        self.0.peek_cursor()
    }
    fn pop_event(&mut self) -> Result<Option<StoredEvent>> {
        self.0.pop_event()
    }
}
struct NativeGcTarget {
    sink: service_backup::GcsSink,
    key: String,
}
impl GcObjectPort for NativeGcTarget {
    type Error = anyhow::Error;
    fn delete(&self) -> Result<()> {
        self.sink.delete_object(&self.key)
    }
}
pub(crate) struct NativeArchiveResources;
impl ArchiveResources for NativeArchiveResources {
    fn fetch_object(&self, uri: &str) -> Result<Vec<u8>> {
        service_backup::fetch_backup_object(uri)
    }
    fn gc_target(&self, uri: &str) -> Result<Box<dyn GcObjectPort<Error = Self::Error>>> {
        let (sink, key) = service_backup::GcsSink::from_exact_uri(uri)?;
        Ok(Box::new(NativeGcTarget { sink, key }))
    }

    fn parse_destination(&self, uri: &str) -> Result<(String, String)> {
        let destination = service_backup::BackupDestination::from_uri(uri)?;
        crate::archive::infrastructure::gcs_uri::gcs_destination(&destination)
    }
    fn archive_dedupe_receipts(
        &self,
        transaction: &mut dyn ArchiveTransactionPort<Error = Self::CatalogError>,
        bucket: &str,
        prefix: &str,
        events: &[StoredEvent],
    ) -> Result<crate::shared_kernel::archive_manifest::ArchiveDedupeReceipt> {
        crate::archive::infrastructure::dedupe_receipt_archive::archive_dedupe_receipts(
            transaction,
            bucket,
            prefix,
            events,
        )
    }

    type Error = anyhow::Error;
    type CatalogError = storage_segment::SegmentError;
    type ObjectError = storage_object::ObjectStoreError;
    fn gcs_store(
        &self,
        bucket: &str,
    ) -> Result<
        Arc<dyn ArchiveObjectPort<Error = Self::ObjectError, CatalogError = Self::CatalogError>>,
        Self::ObjectError,
    > {
        Ok(object_store_port(Arc::new(
            storage_object::GcsObjectStore::new(bucket, "")?,
        )))
    }
    fn catalog_for_uri(
        &self,
        uri: &str,
    ) -> Result<Arc<dyn ArchiveCatalogPort<Error = Self::CatalogError>>> {
        Ok(Arc::new(NativeCatalog(
            crate::archive::infrastructure::archive_catalog::catalog_for_uri(uri)?,
        )))
    }
    fn spill(
        &self,
        parent: &Path,
        name: &str,
    ) -> Result<Box<dyn ArchiveSpillPort<Error = Self::Error, CatalogError = Self::CatalogError>>>
    {
        Ok(Box::new(NativeSpill(SpillCatalog::new(parent, name)?)))
    }
    fn signal_stream(
        &self,
        root: &Path,
        catalog: CatalogReader<Self::CatalogError>,
        signal: SignalKind,
        after: u64,
    ) -> Box<dyn ArchiveSignalPort<Error = Self::Error>> {
        Box::new(NativeSignal(ArchiveSignalStream::new_after(
            root, catalog, signal, after,
        )))
    }
}
