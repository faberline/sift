//! object service over the original resource port.

use crate::archive::application::catalog_service::CatalogService;

use crate::archive::domain::resource_ports::*;
use anyhow::Result;
use std::sync::Arc;

type CatalogError = storage_segment::SegmentError;
type ObjectError = storage_object::ObjectStoreError;
type ObjectPort = dyn ArchiveObjectPort<Error = ObjectError, CatalogError = CatalogError>;
type TransactionPort = dyn ArchiveTransactionPort<Error = CatalogError>;

#[derive(Clone)]
pub(crate) struct ObjectService {
    port: Arc<ObjectPort>,
}
pub(crate) struct FetchedObject {
    pub(crate) bytes: Vec<u8>,
}
pub(crate) struct TransactionService {
    port: Box<TransactionPort>,
}
pub(crate) struct TransactionCommit {
    pub(crate) manifest: CommittedObject,
}
impl ObjectService {
    pub(crate) fn new(port: Arc<ObjectPort>) -> Self {
        Self { port }
    }
    pub(crate) fn get(&self, key: &str) -> Result<FetchedObject, ObjectError> {
        self.port.get(key).map(|bytes| FetchedObject { bytes })
    }
    pub(crate) fn delete(&self, key: &str) -> Result<(), ObjectError> {
        self.port.delete(key)
    }
    pub(crate) fn catalog(&self, prefix: &str) -> Result<CatalogService, CatalogError> {
        self.port.catalog(prefix).map(CatalogService::new)
    }
    pub(crate) fn transaction(&self) -> TransactionService {
        TransactionService {
            port: self.port.transaction(),
        }
    }
}
impl TransactionService {
    pub(crate) fn put(&mut self, object: ArchiveObject) -> Result<(), CatalogError> {
        self.port.put(object)
    }
    pub(crate) fn commit(self, manifest: ArchiveObject) -> Result<TransactionCommit, CatalogError> {
        self.port
            .commit(manifest)
            .map(|manifest| TransactionCommit { manifest })
    }
    pub(crate) fn port_mut(&mut self) -> &mut TransactionPort {
        self.port.as_mut()
    }
}
