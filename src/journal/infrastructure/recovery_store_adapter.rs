//! File-backed recovery primitives; application decides their call order.

use crate::journal::domain::dedupe_store::DedupeStore;
use crate::journal::domain::event_store::RawEventStore;
use crate::journal::domain::journal_head::JournalHead;
use crate::journal::domain::recovery_store::{JournalRoot, RecoveryReader, RecoveryStore};
use crate::journal::domain::wal_store::WalStore;
use crate::journal::infrastructure::canonical_recovery_reader::CanonicalRecoveryReader;
use crate::journal::infrastructure::storage::{
    dedupe_index::DedupeIndex, raw_storage::RawStorage, wal::SignalWal,
};
use crate::node::domain::storage_role::StorageRole;
use crate::node::infrastructure::data_layout::DataLayout;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::shared_kernel::dedupe_stats::DedupeStats;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

pub(crate) struct StorageRecoveryAdapter {
    pub(crate) role: StorageRole,
}
impl JournalRoot for DataLayout {
    fn root(&self) -> &Path {
        DataLayout::root(self)
    }
}
impl RecoveryStore for StorageRecoveryAdapter {
    type Error = anyhow::Error;
    fn initialize_root(&self, root: &Path) -> Result<Arc<dyn JournalRoot>> {
        Ok(Arc::new(DataLayout::open(root, self.role)?))
    }
    fn open_wal(&self, root: &Path) -> Result<Arc<dyn WalStore<Error = Self::Error>>> {
        Ok(Arc::new(SignalWal::open(root)?))
    }
    fn open_events(&self, root: &Path) -> Result<Arc<dyn RawEventStore<Error = Self::Error>>> {
        Ok(Arc::new(RawStorage::open(root)?))
    }
    fn load_head(&self, root: &Path) -> Result<Option<JournalHead>> {
        JournalHead::load(root)
    }
    fn persist_head(&self, root: &Path, head: &JournalHead) -> Result<()> {
        head.persist(root)
    }
    fn open_dedupe(
        &self,
        root: &Path,
    ) -> Result<(Arc<dyn DedupeStore<Error = Self::Error>>, DedupeStats)> {
        let (index, stats) = DedupeIndex::open(root)?;
        Ok((Arc::new(index), stats))
    }
    fn reader<'a>(
        &self,
        storage: &'a dyn RawEventStore<Error = Self::Error>,
        wal: &dyn WalStore<Error = Self::Error>,
        archived: ArchiveWatermarks,
        after: u64,
        repair_segments: bool,
    ) -> Result<Box<dyn RecoveryReader<Error = Self::Error> + 'a>> {
        Ok(Box::new(CanonicalRecoveryReader::open(
            storage,
            wal,
            archived,
            after,
            repair_segments,
        )?))
    }
}
