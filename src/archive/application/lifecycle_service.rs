//! Archive maintenance borrows journal services for each operation.

use crate::archive::application::archive_services::ArchiveServices;
use crate::journal::application::journal_service::JournalService;
use crate::journal::application::raw_event_service::RawEventService;
use crate::shared_kernel::archive_manifest::ArchiveManifest;
use crate::shared_kernel::archive_receipts::{
    ArchiveReceipt, ExpirationReceipt, HotEvictionReceipt, LocalArchiveReceipt,
};
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Arc;

pub(crate) struct ArchiveLifecycleService {
    services: Arc<ArchiveServices>,
}

impl ArchiveLifecycleService {
    pub(crate) fn new(services: Arc<ArchiveServices>) -> Self {
        Self { services }
    }

    pub(crate) fn services(&self) -> &Arc<ArchiveServices> {
        &self.services
    }

    pub(crate) fn cleanup_orphan_spills(&self, root: &Path) -> Result<()> {
        self.services
            .resources
            .cleanup_orphan_spills(root)
            .map(|_| ())
    }

    pub(crate) fn reconcile_staged_gc(&self, root: &Path) -> Result<()> {
        self.services.reconcile_staged_archive_gc(root)
    }

    pub(crate) fn reconcile_retention(
        &self,
        root: &Path,
        storage: &RawEventService,
        generation: u64,
    ) -> Result<Option<u64>> {
        crate::archive::application::reconcile_committed_retention::reconcile_committed_retention(
            &self.services,
            root,
            storage,
            generation,
        )
    }

    pub(crate) fn adopt_receipt(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<ArchiveWatermarks> {
        self.services.adopt_verified_archive_receipt(root, receipt)
    }

    pub(crate) fn resume_local_blob_gc(
        &self,
        journal: &JournalService<'_>,
        max_plan: usize,
        max_scan: usize,
    ) -> Result<(usize, bool)> {
        crate::archive::application::resume_local_blob_gc::resume_local_blob_gc_batch(
            &self.services,
            journal,
            max_plan,
            max_scan,
        )
    }

    pub(crate) fn finish_local_blob_gc(&self, journal: &JournalService<'_>) -> Result<usize> {
        crate::archive::application::resume_local_blob_gc::finish_local_blob_gc(
            &self.services,
            journal,
        )
    }

    pub(crate) fn archive_local(
        &self,
        journal: &JournalService<'_>,
    ) -> Result<LocalArchiveReceipt> {
        crate::archive::application::archive_journal_local::archive_journal_local(
            &self.services,
            journal,
        )
    }

    pub(crate) fn evict_cold(
        &self,
        journal: &JournalService<'_>,
        now: DateTime<Utc>,
    ) -> Result<HotEvictionReceipt> {
        crate::archive::application::evict_cold_segments::evict_committed_cold_segments_at(
            &self.services,
            journal,
            now,
        )
    }

    pub(crate) fn expire_committed(
        &self,
        journal: &JournalService<'_>,
        now: DateTime<Utc>,
    ) -> Result<ExpirationReceipt> {
        crate::archive::application::retention_expiration::expire_committed_events_at(
            &self.services,
            journal,
            now,
        )
    }

    pub(crate) fn restore_gcs(&self, uri: &str, target: &Path) -> Result<ArchiveManifest> {
        crate::archive::application::restore_gcs::restore_gcs(&self.services, uri, target)
    }

    pub(crate) fn install_gc_plan(&self, root: &Path, receipt: &ArchiveReceipt) -> Result<()> {
        self.services.install_archive_gc_plan(root, receipt)
    }

    pub(crate) fn withhold_gc_plan(&self, root: &Path) -> Result<()> {
        self.services.withhold_archive_gc_plan(root)
    }
}
