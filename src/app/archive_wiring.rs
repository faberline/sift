//! Default storage wiring for archive entry points.

use std::sync::Arc;

use crate::archive::application::manifest_validation::ManifestValidator;
use crate::archive::infrastructure::manifest_validation::GcsManifestValidation;

impl Default for ManifestValidator {
    fn default() -> Self {
        Self::new(Arc::new(GcsManifestValidation))
    }
}

use crate::archive::application::archive_services::ArchiveServices;
use crate::archive::application::committed_event_reader::CommittedEventReader;
use crate::journal::application::raw_event_service::RawEventService as RawStorage;
use crate::shared_kernel::archive_manifest::ArchiveManifest;
use crate::shared_kernel::archive_receipts::*;
use crate::shared_kernel::segment_manifest::SegmentManifest;
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::Path;

pub(crate) fn archive_services() -> ArchiveServices {
    ArchiveServices {
        resources: crate::archive::application::resource_factory::ResourceFactory::new(Arc::new(
            crate::archive::infrastructure::resource_adapter::NativeArchiveResources,
        )),
        commit_store: Arc::new(
            crate::archive::infrastructure::commit_store_adapter::NativeCommitStorePort,
        ),
        upload_store: Arc::new(
            crate::archive::infrastructure::upload_store_adapter::NativeUploadStorePort,
        ),
        gc_store: Arc::new(crate::archive::infrastructure::gc_store_adapter::NativeGcStorePort),
        manifest_store: Arc::new(
            crate::archive::infrastructure::manifest_store_adapter::NativeManifestStorePort,
        ),
        segment_codec: Arc::new(
            crate::archive::infrastructure::segment_codec_adapter::NativeSegmentCodecPort,
        ),
        restore_store: Arc::new(
            crate::archive::infrastructure::restore_store_adapter::NativeRestoreStorePort,
        ),
        file_modes: Arc::new(
            crate::archive::infrastructure::file_modes_adapter::NativeFileModesPort,
        ),
    }
}

impl CommittedEventReader {
    pub fn open(root: &Path, after: u64) -> Result<Option<Self>> {
        Self::open_with_services(&archive_services(), root, after)
    }
}
pub fn archive_gcs(storage: &RawStorage, destination_uri: &str) -> Result<ArchiveReceipt> {
    crate::archive::application::archive_raw_storage_to_gcs::archive_gcs(
        &archive_services(),
        storage,
        destination_uri,
    )
}
pub fn archive_journal_gcs(
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    destination_uri: &str,
) -> Result<ArchiveReceipt> {
    crate::archive::application::archive_journal_to_gcs::archive_journal_gcs(
        &archive_services(),
        journal,
        destination_uri,
    )
}
pub fn archive_journal_gcs_captured(
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    destination_uri: &str,
    captured_cursor: u64,
    segments: Vec<(SignalKind, SegmentManifest)>,
) -> Result<ArchiveReceipt> {
    crate::archive::application::archive_journal_to_gcs::archive_journal_gcs_captured(
        &archive_services(),
        journal,
        destination_uri,
        captured_cursor,
        segments,
    )
}
pub fn archive_journal_local(
    journal: &crate::journal::application::journal_service::JournalService<'_>,
) -> Result<LocalArchiveReceipt> {
    crate::archive::application::archive_journal_local::archive_journal_local(
        &archive_services(),
        journal,
    )
}
pub fn archive_journal_local_captured(
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    snapshot_index: u64,
    segments: Vec<(SignalKind, SegmentManifest)>,
) -> Result<LocalArchiveReceipt> {
    crate::archive::application::archive_journal_local::archive_journal_local_captured(
        &archive_services(),
        journal,
        snapshot_index,
        segments,
    )
}
pub fn bootstrap_gcs_if_needed(
    manifest_uri: &str,
    target: impl AsRef<Path>,
) -> Result<Option<ArchiveManifest>> {
    crate::archive::application::restore_gcs::bootstrap_gcs_if_needed(
        &archive_services(),
        manifest_uri,
        target,
    )
}
pub fn committed_status(root: &Path) -> Result<Option<ArchiveCommitStatus>> {
    crate::archive::application::archive_status_queries::committed_status(&archive_services(), root)
}
pub fn evict_committed_cold_segments_at(
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    now: DateTime<Utc>,
) -> Result<HotEvictionReceipt> {
    crate::archive::application::evict_cold_segments::evict_committed_cold_segments_at(
        &archive_services(),
        journal,
        now,
    )
}
pub fn expire_committed_events_at(
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    now: DateTime<Utc>,
) -> Result<ExpirationReceipt> {
    crate::archive::application::retention_expiration::expire_committed_events_at(
        &archive_services(),
        journal,
        now,
    )
}
pub fn finalize_archive_gc_after_checkpoint(root: &Path) -> Result<usize> {
    crate::archive::application::finalize_archive_gc::finalize_archive_gc_after_checkpoint(
        &archive_services(),
        root,
    )
}
pub fn finalize_archive_gc_batch_after_checkpoint(
    root: &Path,
    max_objects: usize,
) -> Result<(usize, bool)> {
    crate::archive::application::finalize_archive_gc::finalize_archive_gc_batch_after_checkpoint(
        &archive_services(),
        root,
        max_objects,
    )
}
pub fn replay_committed_events<F>(
    root: &Path,
    signal: SignalKind,
    project: &str,
    environment: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
    visitor: F,
) -> Result<Option<ArchiveReplay>>
where
    F: FnMut(StoredEvent) -> Result<()>,
{
    crate::archive::application::replay_committed_events::replay_committed_events(
        &archive_services(),
        root,
        signal,
        project,
        environment,
        start,
        end,
        visitor,
    )
}
pub fn restore_gcs(manifest_uri: &str, target: impl AsRef<Path>) -> Result<ArchiveManifest> {
    crate::archive::application::restore_gcs::restore_gcs(&archive_services(), manifest_uri, target)
}
pub fn resume_local_blob_gc_batch(
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    max_plan_entries: usize,
    max_scan_events: usize,
) -> Result<(usize, bool)> {
    crate::archive::application::resume_local_blob_gc::resume_local_blob_gc_batch(
        &archive_services(),
        journal,
        max_plan_entries,
        max_scan_events,
    )
}
pub fn retention_due_at(root: &Path, now: DateTime<Utc>) -> Result<bool> {
    crate::archive::application::retention_due::retention_due_at(&archive_services(), root, now)
}
pub fn verify_committed_manifest_available(root: &Path) -> Result<bool> {
    crate::archive::application::archive_status_queries::verify_committed_manifest_available(
        &archive_services(),
        root,
    )
}

pub fn inspect_archive_catalog(
    manifest: &crate::shared_kernel::archive_manifest::ArchiveManifest,
) -> Result<(
    Vec<crate::shared_kernel::archive_manifest::ArchiveSegment>,
    Vec<crate::shared_kernel::archive_manifest::ArchiveBlob>,
)> {
    crate::archive::application::inspect_archive_catalog::inspect_archive_catalog(
        &archive_services(),
        manifest,
    )
}
pub fn inspect_archive_gc_plan(
    manifest: &crate::shared_kernel::archive_manifest::ArchiveManifest,
) -> Result<Vec<String>> {
    crate::archive::application::inspect_archive_catalog::inspect_archive_gc_plan(
        &archive_services(),
        manifest,
    )
}

#[cfg(test)]
mod tests;
