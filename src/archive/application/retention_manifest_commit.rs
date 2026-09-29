//! Checking a bounded retention manifest against its source and committing it.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::archive::application::archive_receipts::{ArchiveReceipt, ExpirationReceipt};
use crate::archive::application::evict_cold_segments::evict_committed_cold_segments_at;
use crate::archive::application::resume_local_blob_gc::resume_local_blob_gc_batch;
use crate::archive::domain::archive_manifest::ArchiveManifest;
use crate::archive::domain::event_set_digest::decode_event_content_digest;
use crate::archive::infrastructure::archive_commit_state::{
    record_archive_commit, ArchiveCommitState,
};
use crate::archive::infrastructure::archive_gc_pending_store::{
    promote_archive_gc_pending, stage_archive_gc_pending,
};

pub(super) fn validate_retention_receipt_source(
    receipt: &ArchiveReceipt,
    source: &ArchiveCommitState,
) -> Result<()> {
    let delta = receipt
        .manifest
        .retention_delta
        .as_ref()
        .context("bounded retention manifest is missing its source delta")?;
    if delta.source_manifest_uri != source.manifest_uri
        || delta.source_manifest_sha256 != source.manifest_sha256
        || delta.source_generation != source.manifest.retention_generation
        || delta.source_event_count != source.manifest.event_count
        || delta.source_event_content_sha256 != source.manifest.event_content_sha256
        || receipt.manifest.retention_generation != delta.source_generation.saturating_add(1)
        || receipt.manifest.raft_snapshot_index != source.manifest.raft_snapshot_index
        || receipt.manifest.watermarks != source.manifest.watermarks
    {
        bail!("bounded retention manifest source identity changed");
    }
    Ok(())
}

pub(super) fn finish_bounded_retention_commit(
    journal: &crate::journal::infrastructure::durable_journal::DurableJournal,
    source: &ArchiveManifest,
    receipt: ArchiveReceipt,
) -> Result<ExpirationReceipt> {
    let delta = receipt
        .manifest
        .retention_delta
        .clone()
        .context("bounded retention receipt is missing its delta")?;
    stage_archive_gc_pending(journal.storage().root(), &receipt)?;
    journal.mark_recovery_required();
    record_archive_commit(journal.storage().root(), &receipt)?;
    promote_archive_gc_pending(journal.storage().root(), &receipt)?;
    let cutoff = DateTime::<Utc>::from_timestamp_nanos(delta.cutoff_unix_nano);
    journal
        .storage()
        .evict_expired_before(cutoff, receipt.manifest.raft_snapshot_index)?;
    journal.apply_expiration_head(
        cutoff,
        delta.source_event_count,
        receipt.manifest.event_count,
        decode_event_content_digest(&delta.source_event_content_sha256)?,
        decode_event_content_digest(&receipt.manifest.event_content_sha256)?,
        false,
    )?;
    resume_local_blob_gc_batch(journal, 128, 1_280_000)?;
    evict_committed_cold_segments_at(journal, cutoff + chrono::Duration::days(180))?;
    Ok(ExpirationReceipt {
        manifest_uri: receipt.manifest_uri,
        retained_events: receipt.manifest.event_count,
        retained_segments: usize::try_from(receipt.manifest.segment_count).unwrap_or(usize::MAX),
        expired_events: source
            .event_count
            .saturating_sub(receipt.manifest.event_count),
        replaced_segments: 0,
        removed_segments: 0,
    })
}
