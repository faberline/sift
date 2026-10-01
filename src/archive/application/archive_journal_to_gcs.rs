//! Committing a remote archive of the journal before any canonical WAL bytes
//! are removed.

use crate::archive::application::archive_services::ArchiveServices;

use anyhow::Result;

use crate::archive::application::archive_raw_storage_to_gcs::archive_gcs_captured;
use crate::archive::application::archive_status_queries::committed_watermarks;
use crate::shared_kernel::archive_receipts::ArchiveReceipt;
use crate::shared_kernel::segment_manifest::SegmentManifest;
use crate::SignalKind;

/// Commit a remote archive locally before any canonical WAL bytes are removed.
pub(crate) fn archive_journal_gcs(
    services: &ArchiveServices,
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    destination_uri: &str,
) -> Result<ArchiveReceipt> {
    let (captured_cursor, segments) = journal.seal_archive_prefix()?;
    archive_journal_gcs_captured(
        services,
        journal,
        destination_uri,
        captured_cursor,
        segments,
    )
}

#[doc(hidden)]
pub(crate) fn archive_journal_gcs_captured(
    services: &ArchiveServices,
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    destination_uri: &str,
    captured_cursor: u64,
    segments: Vec<(SignalKind, SegmentManifest)>,
) -> Result<ArchiveReceipt> {
    let receipt = archive_gcs_captured(
        services,
        journal.storage(),
        destination_uri,
        captured_cursor,
        segments,
    )?;
    services.stage_archive_gc_pending(journal.storage().root(), &receipt)?;
    let watermarks = services.record_archive_commit(journal.storage().root(), &receipt)?;
    services.promote_archive_gc_pending(journal.storage().root(), &receipt)?;
    services.retire_local_commit(journal.storage().root())?;
    journal.compact_archived_wal(watermarks)?;
    services.clear_archive_upload_intent(journal.storage().root(), &receipt)?;
    Ok(receipt)
}

/// Retry WAL removal that is already authorized by a durable local or remote
/// commit. This is safe to call after a crash between receipt commit and WAL
/// truncation.
pub(crate) fn reconcile_committed_wal(
    services: &ArchiveServices,
    journal: &crate::journal::application::journal_service::JournalService<'_>,
) -> Result<()> {
    journal.compact_archived_wal(committed_watermarks(services, journal.storage().root())?)
}
