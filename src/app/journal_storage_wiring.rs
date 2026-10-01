//! Wire storage, archive and checkpoint ports without a journal ownership cycle.
use crate::journal::application::{
    journal_dependencies::JournalDependencies,
    journal_runtime::JournalRuntime,
    journal_service::{DurableJournal, JournalService},
    raw_event_service::RawEventService,
};
use crate::journal::domain::{
    archive_access::{ArchiveMaintenance, ArchiveRead, ArchivedEventReader},
    command_submission::JournalGovernance,
    event_store::RawEventStore,
    journal_api::JournalApi,
    snapshot_ports::JournalStaging,
};
use crate::node::domain::storage_role::StorageRole;
use crate::shared_kernel::{
    archive_manifest::ArchiveManifest, archive_receipts::*, archive_watermarks::ArchiveWatermarks,
    dedupe_receipt::DedupeReceipt, stored_event::StoredEvent,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Arc;

pub(super) fn open_journal(
    root: impl AsRef<Path>,
    governance: Arc<dyn JournalGovernance<Error = anyhow::Error>>,
    role: StorageRole,
    resident_limit: usize,
) -> Result<DurableJournal> {
    Ok(JournalService::owned(Arc::new(
        JournalRuntime::open_configured(root, governance, resident_limit, dependencies(role))?,
    )))
}

fn dependencies(role: StorageRole) -> JournalDependencies {
    JournalDependencies {
        resources: Arc::new(
            crate::journal::infrastructure::recovery_store_adapter::StorageRecoveryAdapter { role },
        ),
        archive_read: Arc::new(ArchiveReads),
        archive_maintenance: Arc::new(ArchiveMaintenanceBridge),
        snapshots: Arc::new(
            crate::journal::infrastructure::snapshot_codec_adapter::JournalSnapshotCodec,
        ),
        staging: Arc::new(Staging),
    }
}
struct Staging;
impl JournalStaging for Staging {
    type Error = anyhow::Error;
    fn open(&self, root: &Path) -> Result<Arc<dyn JournalApi<Error = Self::Error>>> {
        Ok(DurableJournal::open(root)?.port())
    }
}
struct ArchiveReads;
impl ArchiveRead for ArchiveReads {
    type Error = anyhow::Error;
    fn committed_watermarks(&self, root: &Path) -> Result<ArchiveWatermarks> {
        crate::archive::application::archive_status_queries::committed_watermarks(root)
    }
    fn committed_status(&self, root: &Path) -> Result<Option<ArchiveCommitStatus>> {
        crate::archive::application::archive_status_queries::committed_status(root)
    }
    fn local_committed_status(&self, root: &Path) -> Result<Option<LocalArchiveStatus>> {
        crate::archive::application::archive_status_queries::local_committed_status(root)
    }
    fn remote_retained_state(&self, root: &Path) -> Result<Option<RemoteRetainedState>> {
        crate::archive::application::archive_status_queries::remote_retained_state(root)
    }
    fn read_committed_events_after(
        &self,
        root: &Path,
        after: u64,
        limit: usize,
    ) -> Result<Option<Vec<StoredEvent>>> {
        crate::archive::application::replay_committed_events::read_committed_events_after(
            root, after, limit,
        )
    }
    fn open_reader(
        &self,
        root: &Path,
        after: u64,
    ) -> Result<Option<Box<dyn ArchivedEventReader<Error = Self::Error>>>> {
        Ok(
            crate::archive::application::committed_event_reader::CommittedEventReader::open(
                root, after,
            )?
            .map(|r| Box::new(r) as Box<dyn ArchivedEventReader<Error = Self::Error>>),
        )
    }
    fn replay_recent_events(
        &self,
        root: &Path,
        since: DateTime<Utc>,
        visitor: &mut dyn FnMut(StoredEvent) -> Result<()>,
    ) -> Result<Option<ArchiveReplay>> {
        crate::archive::application::replay_recent_committed::replay_recent_committed_events(
            root, since, visitor,
        )
    }
    fn replay_recent_receipts(
        &self,
        root: &Path,
        since: DateTime<Utc>,
        visitor: &mut dyn FnMut(DedupeReceipt) -> Result<()>,
    ) -> Result<Option<u64>> {
        crate::archive::application::replay_recent_committed::replay_recent_committed_receipts(
            root, since, visitor,
        )
    }
    fn validate_manifest(&self, manifest: &ArchiveManifest) -> Result<()> {
        Ok(crate::archive::application::manifest_validation::validate_archive_manifest(manifest)?)
    }
}
impl ArchivedEventReader
    for crate::archive::application::committed_event_reader::CommittedEventReader
{
    type Error = anyhow::Error;
    fn snapshot_index(&self) -> u64 {
        self.snapshot_index()
    }
    fn read_next(&mut self, limit: usize) -> Result<Vec<StoredEvent>> {
        self.read_next(limit)
    }
}
struct ArchiveMaintenanceBridge;
impl ArchiveMaintenance for ArchiveMaintenanceBridge {
    type Error = anyhow::Error;
    fn cleanup_orphan_spills(&self, root: &Path) -> Result<()> {
        crate::archive::infrastructure::spill_catalog::cleanup_orphan_spills(root).map(|_| ())
    }
    fn reconcile_staged_gc(&self, root: &Path) -> Result<()> {
        crate::archive::infrastructure::archive_gc_pending_store::reconcile_staged_archive_gc(root)
    }
    fn reconcile_retention(
        &self,
        root: &Path,
        storage: Arc<dyn RawEventStore<Error = Self::Error>>,
        generation: u64,
    ) -> Result<Option<u64>> {
        crate::archive::application::reconcile_committed_retention::reconcile_committed_retention(
            root,
            &RawEventService::new(storage),
            generation,
        )
    }
    fn adopt_receipt(&self, root: &Path, receipt: &ArchiveReceipt) -> Result<ArchiveWatermarks> {
        crate::archive::infrastructure::archive_commit_state::adopt_verified_archive_receipt(
            root, receipt,
        )
    }
    fn resume_local_blob_gc(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
        max_plan: usize,
        max_scan: usize,
    ) -> Result<(usize, bool)> {
        crate::archive::application::resume_local_blob_gc::resume_local_blob_gc_batch(
            &JournalService::borrowed(journal),
            max_plan,
            max_scan,
        )
    }
    fn finish_local_blob_gc(&self, journal: &dyn JournalApi<Error = Self::Error>) -> Result<usize> {
        crate::archive::application::resume_local_blob_gc::finish_local_blob_gc(
            &JournalService::borrowed(journal),
        )
    }
    fn archive_local(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
    ) -> Result<LocalArchiveReceipt> {
        crate::archive::application::archive_journal_local::archive_journal_local(
            &JournalService::borrowed(journal),
        )
    }
    fn evict_cold(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
        now: DateTime<Utc>,
    ) -> Result<HotEvictionReceipt> {
        crate::archive::application::evict_cold_segments::evict_committed_cold_segments_at(
            &JournalService::borrowed(journal),
            now,
        )
    }
    fn expire_committed(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
        now: DateTime<Utc>,
    ) -> Result<ExpirationReceipt> {
        crate::archive::application::retention_expiration::expire_committed_events_at(
            &JournalService::borrowed(journal),
            now,
        )
    }
    fn restore_gcs(&self, uri: &str, target: &Path) -> Result<ArchiveManifest> {
        crate::archive::application::restore_gcs::restore_gcs(uri, target)
    }
    fn install_gc_plan(&self, root: &Path, receipt: &ArchiveReceipt) -> Result<()> {
        crate::archive::infrastructure::archive_gc_pending_store::install_archive_gc_plan(
            root, receipt,
        )
    }
    fn withhold_gc_plan(&self, root: &Path) -> Result<()> {
        crate::archive::infrastructure::archive_gc_pending_store::withhold_archive_gc_plan(root)
    }
}
impl RawEventService {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::new(Arc::new(
            crate::journal::infrastructure::storage::raw_storage::RawStorage::open(root)?,
        )))
    }
    pub fn open_with_config(
        root: impl AsRef<Path>,
        config: crate::journal::domain::storage_config::StorageConfig,
    ) -> Result<Self> {
        Ok(Self::new(Arc::new(
            crate::journal::infrastructure::storage::raw_storage::RawStorage::open_with_config(
                root, config,
            )?,
        )))
    }
}
impl crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine {
    pub fn new(journal: Arc<DurableJournal>) -> Self {
        let root = journal.data_dir().to_path_buf();
        Self::open(root, journal).expect("open Sift state-machine control state")
    }
    pub fn open(root: impl AsRef<Path>, journal: Arc<DurableJournal>) -> Result<Self> {
        Self::open_backend(root, journal.port())
    }
}

#[cfg(test)]
mod tests;
