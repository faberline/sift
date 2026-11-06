//! Opening stores and persisting their durable head preserves recovery order.

use crate::journal::domain::dedupe_store::DedupeStore;
use crate::journal::domain::event_store::RawEventStore;
use crate::journal::domain::journal_head::JournalHead;
use crate::journal::domain::wal_store::WalStore;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::shared_kernel::dedupe_stats::DedupeStats;
use crate::shared_kernel::stored_event::StoredEvent;
use std::path::Path;
use std::sync::Arc;

pub(crate) trait JournalRoot: Send + Sync {
    fn root(&self) -> &Path;
}
pub(crate) trait RecoveryReader: Send {
    type Error;
    fn read_page(&mut self) -> Result<Vec<StoredEvent>, Self::Error>;
    fn read_page_with_limits(
        &mut self,
        max_events: usize,
        max_bytes: usize,
    ) -> Result<(Vec<StoredEvent>, bool), Self::Error>;
}

pub(crate) type RecoveredDedupe<E> = (Arc<dyn DedupeStore<Error = E>>, DedupeStats);
pub(crate) trait RecoveryStore: Send + Sync {
    type Error;
    fn initialize_root(&self, root: &Path) -> Result<Arc<dyn JournalRoot>, Self::Error>;
    fn open_wal(&self, root: &Path) -> Result<Arc<dyn WalStore<Error = Self::Error>>, Self::Error>;
    fn open_events(
        &self,
        root: &Path,
    ) -> Result<Arc<dyn RawEventStore<Error = Self::Error>>, Self::Error>;
    fn load_head(&self, root: &Path) -> Result<Option<JournalHead>, Self::Error>;
    fn persist_head(&self, root: &Path, head: &JournalHead) -> Result<(), Self::Error>;
    fn open_dedupe(&self, root: &Path) -> Result<RecoveredDedupe<Self::Error>, Self::Error>;
    fn reader<'a>(
        &self,
        storage: &'a dyn RawEventStore<Error = Self::Error>,
        wal: &dyn WalStore<Error = Self::Error>,
        archived: ArchiveWatermarks,
        after: u64,
        repair_segments: bool,
    ) -> Result<Box<dyn RecoveryReader<Error = Self::Error> + 'a>, Self::Error>;
}
