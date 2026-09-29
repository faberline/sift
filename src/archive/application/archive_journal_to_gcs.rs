//! Committing a remote archive of the journal before any canonical WAL bytes
//! are removed.

use anyhow::Result;

use crate::archive::application::archive_raw_storage_to_gcs::archive_gcs_captured;
use crate::archive::application::archive_receipts::ArchiveReceipt;
use crate::archive::application::archive_status_queries::committed_watermarks;
use crate::archive::infrastructure::archive_commit_state::record_archive_commit;
use crate::archive::infrastructure::archive_gc_pending_store::{
    promote_archive_gc_pending, stage_archive_gc_pending,
};
use crate::archive::infrastructure::archive_upload_intent::clear_archive_upload_intent;
use crate::archive::infrastructure::local_segment_commit_state::retire_local_commit;
use crate::storage::SegmentManifest;
use crate::SignalKind;

/// Commit a remote archive locally before any canonical WAL bytes are removed.
pub fn archive_journal_gcs(
    journal: &crate::journal::infrastructure::durable_journal::DurableJournal,
    destination_uri: &str,
) -> Result<ArchiveReceipt> {
    let (captured_cursor, segments) = journal.seal_archive_prefix()?;
    archive_journal_gcs_captured(journal, destination_uri, captured_cursor, segments)
}

#[doc(hidden)]
pub fn archive_journal_gcs_captured(
    journal: &crate::journal::infrastructure::durable_journal::DurableJournal,
    destination_uri: &str,
    captured_cursor: u64,
    segments: Vec<(SignalKind, SegmentManifest)>,
) -> Result<ArchiveReceipt> {
    let receipt = archive_gcs_captured(
        journal.storage(),
        destination_uri,
        captured_cursor,
        segments,
    )?;
    stage_archive_gc_pending(journal.storage().root(), &receipt)?;
    let watermarks = record_archive_commit(journal.storage().root(), &receipt)?;
    promote_archive_gc_pending(journal.storage().root(), &receipt)?;
    retire_local_commit(journal.storage().root())?;
    journal.compact_archived_wal(watermarks)?;
    clear_archive_upload_intent(journal.storage().root(), &receipt)?;
    Ok(receipt)
}

/// Retry WAL removal that is already authorized by a durable local or remote
/// commit. This is safe to call after a crash between receipt commit and WAL
/// truncation.
pub(crate) fn reconcile_committed_wal(
    journal: &crate::journal::infrastructure::durable_journal::DurableJournal,
) -> Result<()> {
    journal.compact_archived_wal(committed_watermarks(journal.storage().root())?)
}
