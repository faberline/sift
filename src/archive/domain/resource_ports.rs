//! Archive resource ports. The application owns lifecycle decisions.

use crate::shared_kernel::catalog_manifest::{CatalogPageRef, CatalogRoot};
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CatalogEntry {
    pub(crate) key: String,
    pub(crate) value: Vec<u8>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CatalogMutation {
    pub(crate) root: CatalogRoot,
    pub(crate) written_page_keys: Vec<String>,
    pub(crate) obsolete_page_keys: Vec<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CatalogBuild {
    pub(crate) root: CatalogRoot,
    pub(crate) written_page_count: u64,
    pub(crate) peak_buffer_bytes: usize,
}
pub(crate) type CatalogReader<E> = Box<dyn Iterator<Item = Result<CatalogEntry, E>> + Send>;
pub(crate) type PageKeys<E> = Box<dyn Iterator<Item = Result<String, E>> + Send>;

pub(crate) trait ArchiveCatalogPort: Send + Sync {
    type Error;
    fn reader(&self, root: &CatalogRoot) -> Result<CatalogReader<Self::Error>, Self::Error>;
    fn reader_after(
        &self,
        root: &CatalogRoot,
        after: &str,
    ) -> Result<CatalogReader<Self::Error>, Self::Error>;
    fn page_keys(&self, root: &CatalogRoot) -> Result<PageKeys<Self::Error>, Self::Error>;
    fn lookup(&self, root: &CatalogRoot, key: &str) -> Result<Option<CatalogEntry>, Self::Error>;
    fn last_with_prefix(
        &self,
        root: &CatalogRoot,
        prefix: &str,
    ) -> Result<Option<CatalogEntry>, Self::Error>;
    fn remove(&self, root: &CatalogRoot, key: &str) -> Result<CatalogMutation, Self::Error>;
    fn upsert(
        &self,
        root: &CatalogRoot,
        entry: CatalogEntry,
    ) -> Result<CatalogMutation, Self::Error>;
    fn build_sorted_observed(
        &self,
        entries: &mut dyn Iterator<Item = Result<CatalogEntry, Self::Error>>,
        observed: &mut dyn FnMut(&CatalogPageRef) -> Result<(), Self::Error>,
    ) -> Result<CatalogBuild, Self::Error>;
}

pub(crate) trait ArchiveSpillPort: Send {
    type Error;
    type CatalogError;
    fn root(&self) -> CatalogRoot;
    fn upsert(&mut self, key: String, value: Vec<u8>) -> Result<(), Self::Error>;
    fn lookup(&self, key: &str) -> Result<Option<Vec<u8>>, Self::Error>;
    fn remove(&mut self, key: &str) -> Result<(), Self::Error>;
    fn reader(&self) -> Result<CatalogReader<Self::CatalogError>, Self::Error>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArchiveObject {
    pub(crate) key: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) content_type: String,
}
impl ArchiveObject {
    pub(crate) fn new(
        key: impl Into<String>,
        bytes: Vec<u8>,
        content_type: impl Into<String>,
    ) -> Self {
        Self {
            key: key.into(),
            bytes,
            content_type: content_type.into(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommittedObject {
    pub(crate) key: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
}
pub(crate) trait ArchiveTransactionPort: Send {
    type Error;
    fn put(&mut self, object: ArchiveObject) -> Result<(), Self::Error>;
    fn commit(self: Box<Self>, manifest: ArchiveObject) -> Result<CommittedObject, Self::Error>;
}

pub(crate) trait ArchiveObjectPort: Send + Sync {
    type Error;
    type CatalogError;
    fn get(&self, key: &str) -> Result<Vec<u8>, Self::Error>;
    fn delete(&self, key: &str) -> Result<(), Self::Error>;
    fn catalog(
        &self,
        prefix: &str,
    ) -> Result<Arc<dyn ArchiveCatalogPort<Error = Self::CatalogError>>, Self::CatalogError>;
    fn transaction(&self) -> Box<dyn ArchiveTransactionPort<Error = Self::CatalogError>>;
}

pub(crate) trait ArchiveSignalPort: Send {
    type Error;
    fn peek_cursor(&mut self) -> Result<Option<u64>, Self::Error>;
    fn pop_event(&mut self) -> Result<Option<StoredEvent>, Self::Error>;
}

pub(crate) trait GcObjectPort: Send + Sync {
    type Error;
    fn delete(&self) -> Result<(), Self::Error>;
}
pub(crate) type ObjectResource<E, C> = Arc<dyn ArchiveObjectPort<Error = E, CatalogError = C>>;
pub(crate) type SpillResource<E, C> = Box<dyn ArchiveSpillPort<Error = E, CatalogError = C>>;

pub(crate) trait ArchiveResources: Send + Sync {
    fn cleanup_orphan_spills(&self, root: &Path) -> Result<usize, Self::Error>;
    fn fetch_object(&self, uri: &str) -> Result<Vec<u8>, Self::Error>;
    fn gc_target(
        &self,
        uri: &str,
    ) -> Result<Box<dyn GcObjectPort<Error = Self::Error>>, Self::Error>;

    fn parse_destination(&self, uri: &str) -> Result<(String, String), Self::Error>;
    fn archive_dedupe_receipts(
        &self,
        transaction: &mut dyn ArchiveTransactionPort<Error = Self::CatalogError>,
        bucket: &str,
        prefix: &str,
        events: &[StoredEvent],
    ) -> Result<crate::shared_kernel::archive_manifest::ArchiveDedupeReceipt, Self::Error>;

    type Error;
    type CatalogError;
    type ObjectError;
    fn gcs_store(
        &self,
        bucket: &str,
    ) -> Result<ObjectResource<Self::ObjectError, Self::CatalogError>, Self::ObjectError>;
    fn catalog_for_uri(
        &self,
        uri: &str,
    ) -> Result<Arc<dyn ArchiveCatalogPort<Error = Self::CatalogError>>, Self::Error>;
    fn spill(
        &self,
        parent: &Path,
        name: &str,
    ) -> Result<SpillResource<Self::Error, Self::CatalogError>, Self::Error>;
    fn signal_stream(
        &self,
        root: &Path,
        catalog: CatalogReader<Self::CatalogError>,
        signal: SignalKind,
        after: u64,
    ) -> Box<dyn ArchiveSignalPort<Error = Self::Error>>;
}
