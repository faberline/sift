//! Archive reads and maintenance have separate ports and no owned journal link.

use crate::journal::domain::event_store::RawEventStore;
use crate::journal::domain::journal_api::JournalApi;
use crate::shared_kernel::archive_manifest::ArchiveManifest;
use crate::shared_kernel::archive_receipts::{
    ArchiveCommitStatus, ArchiveReceipt, ArchiveReplay, ExpirationReceipt, HotEvictionReceipt,
    LocalArchiveReceipt, LocalArchiveStatus, RemoteRetainedState,
};
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::shared_kernel::dedupe_receipt::DedupeReceipt;
use crate::shared_kernel::stored_event::StoredEvent;
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Arc;

pub(crate) trait ArchivedEventReader: Send {
    type Error;
    fn snapshot_index(&self) -> u64;
    fn read_next(&mut self, limit: usize) -> Result<Vec<StoredEvent>, Self::Error>;
}

pub(crate) type ArchivedReader<E> = Box<dyn ArchivedEventReader<Error = E>>;

pub(crate) trait ArchiveRead: Send + Sync {
    type Error;
    fn committed_watermarks(&self, root: &Path) -> Result<ArchiveWatermarks, Self::Error>;
    fn committed_status(&self, root: &Path) -> Result<Option<ArchiveCommitStatus>, Self::Error>;
    fn local_committed_status(
        &self,
        root: &Path,
    ) -> Result<Option<LocalArchiveStatus>, Self::Error>;
    fn remote_retained_state(
        &self,
        root: &Path,
    ) -> Result<Option<RemoteRetainedState>, Self::Error>;
    fn read_committed_events_after(
        &self,
        root: &Path,
        after: u64,
        limit: usize,
    ) -> Result<Option<Vec<StoredEvent>>, Self::Error>;
    fn open_reader(
        &self,
        root: &Path,
        after: u64,
    ) -> Result<Option<ArchivedReader<Self::Error>>, Self::Error>;
    fn replay_recent_events(
        &self,
        root: &Path,
        since: DateTime<Utc>,
        visitor: &mut dyn FnMut(StoredEvent) -> Result<(), Self::Error>,
    ) -> Result<Option<ArchiveReplay>, Self::Error>;
    fn replay_recent_receipts(
        &self,
        root: &Path,
        since: DateTime<Utc>,
        visitor: &mut dyn FnMut(DedupeReceipt) -> Result<(), Self::Error>,
    ) -> Result<Option<u64>, Self::Error>;
    fn validate_manifest(&self, manifest: &ArchiveManifest) -> Result<(), Self::Error>;
}

pub(crate) trait ArchiveMaintenance: Send + Sync {
    type Error;
    fn cleanup_orphan_spills(&self, root: &Path) -> Result<(), Self::Error>;
    fn reconcile_staged_gc(&self, root: &Path) -> Result<(), Self::Error>;
    fn reconcile_retention(
        &self,
        root: &Path,
        storage: Arc<dyn RawEventStore<Error = Self::Error>>,
        generation: u64,
    ) -> Result<Option<u64>, Self::Error>;
    fn adopt_receipt(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<ArchiveWatermarks, Self::Error>;
    fn resume_local_blob_gc(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
        max_plan: usize,
        max_scan: usize,
    ) -> Result<(usize, bool), Self::Error>;
    fn finish_local_blob_gc(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
    ) -> Result<usize, Self::Error>;
    fn archive_local(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
    ) -> Result<LocalArchiveReceipt, Self::Error>;
    fn evict_cold(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
        now: DateTime<Utc>,
    ) -> Result<HotEvictionReceipt, Self::Error>;
    fn expire_committed(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
        now: DateTime<Utc>,
    ) -> Result<ExpirationReceipt, Self::Error>;
    fn restore_gcs(&self, uri: &str, target: &Path) -> Result<ArchiveManifest, Self::Error>;
    fn install_gc_plan(&self, root: &Path, receipt: &ArchiveReceipt) -> Result<(), Self::Error>;
    fn withhold_gc_plan(&self, root: &Path) -> Result<(), Self::Error>;
}
