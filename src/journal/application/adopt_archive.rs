//! Adopting a committed archive checkpoint, retention delta and coverage.

use std::sync::atomic::Ordering;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::journal::application::dedupe_window::append_unique_dedupe_page;
use crate::journal::domain::event_content_digest::xor_event_content_digest;
use crate::journal::domain::journal_limits::RECOVERY_PAGE_EVENTS;
use crate::journal::domain::journal_state::JournalState;
use crate::journal::infrastructure::canonical_recovery_reader::CanonicalRecoveryReader;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::shared_kernel::event_content_digest::{decode_digest, xor_digest};

impl DurableJournal {
    /// Adopt a hash-verified archive checkpoint on a caught-up replica.
    ///
    /// The journal write lock also blocks queries and appends. The local
    /// segment prefix is rewritten before the archive receipt permits WAL
    /// compaction. A new source generation then forces every typed projection
    /// to rebuild from the retained canonical rows.
    pub(crate) fn adopt_archive_checkpoint(
        &self,
        restored: &DurableJournal,
        receipt: &crate::storage::archive::ArchiveReceipt,
        expected_raw_cursor: u64,
    ) -> Result<()> {
        if receipt.manifest.raft_snapshot_index != expected_raw_cursor
            || restored.last_cursor() != expected_raw_cursor
            || restored.total_event_count() != receipt.manifest.event_count
        {
            bail!("verified archive checkpoint does not match its Raft cursor");
        }

        self.recovery_required.store(true, Ordering::Release);
        let mut state = self.state.write().expect("journal state lock poisoned");
        self.dedupe.preflight_rebuild()?;
        let staged_dedupe = self.dedupe.replace_from(&restored.dedupe)?;
        if staged_dedupe.indexed_through_cursor != expected_raw_cursor
            || staged_dedupe.newest_cursor > expected_raw_cursor
            || staged_dedupe.rebuild_required
        {
            bail!("validated archive checkpoint dedupe index disagrees with its manifest");
        }
        let last_cursor = state.last_cursor.max(expected_raw_cursor);
        let prior_archive = crate::storage::archive::committed_status(self.data_dir())?;
        let archive_identity_changed = prior_archive.as_ref().is_some_and(|status| {
            status.manifest_uri != receipt.manifest_uri
                || status.manifest_sha256 != receipt.manifest_sha256
        });
        self.storage
            .reconcile_retained_prefix(restored.storage(), receipt.manifest.raft_snapshot_index)?;
        let watermarks =
            crate::storage::archive::adopt_verified_archive_receipt(self.data_dir(), receipt)?;
        self.wal.compact_through(watermarks)?;

        let projection_generation = if receipt.manifest.retention_scan.is_none()
            && (receipt.manifest.retention_generation > state.retention_generation
                || archive_identity_changed)
        {
            state.projection_generation.saturating_add(1)
        } else {
            state.projection_generation
        };
        let mut rebuilt = JournalState {
            projection_generation,
            retention_generation: receipt.manifest.retention_generation,
            ..JournalState::default()
        };
        let mut recovery =
            CanonicalRecoveryReader::open(&self.storage, &self.wal, watermarks, 0, false)?;
        let mut local_after_archive = 0_u64;
        let mut local_after_archive_digest = [0_u8; 32];
        let mut suffix_dedupe_page = Vec::with_capacity(RECOVERY_PAGE_EVENTS);
        loop {
            let page = recovery.read_page()?;
            if page.is_empty() {
                break;
            }
            for event in page {
                if !watermarks.covers(event.event.signal, event.cursor) {
                    local_after_archive = local_after_archive.saturating_add(1);
                    xor_event_content_digest(&mut local_after_archive_digest, &event.event)?;
                    suffix_dedupe_page.push(event.clone());
                    if suffix_dedupe_page.len() == RECOVERY_PAGE_EVENTS {
                        append_unique_dedupe_page(&self.dedupe, &mut suffix_dedupe_page)?;
                    }
                }
                Self::insert_recovered(&mut rebuilt, event, self.resident_limit)?;
            }
        }
        append_unique_dedupe_page(&self.dedupe, &mut suffix_dedupe_page)?;
        rebuilt.last_cursor = rebuilt.last_cursor.max(last_cursor);
        let retained_events = receipt
            .manifest
            .event_count
            .checked_add(local_after_archive)
            .context("retained event count exhausted u64")?;
        rebuilt.total_events = retained_events;
        let archived_digest: [u8; 32] = hex::decode(&receipt.manifest.event_content_sha256)
            .context("decode adopted archive event content digest")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("archive event content digest must be 32 bytes"))?;
        rebuilt.event_content_digest = xor_digest(archived_digest, local_after_archive_digest);
        crate::storage::JournalHead::new(rebuilt.last_cursor, retained_events)
            .with_projection_generation(projection_generation)
            .with_retention_generation(rebuilt.retention_generation)
            .persist(self.data_dir())?;
        *state = rebuilt;
        drop(state);
        crate::storage::archive::resume_local_blob_gc_batch(self, 128, 1_280_000)?;
        self.recovery_required.store(false, Ordering::Release);
        Ok(())
    }

    /// Apply one manifest-backed retention generation to a caught-up voter.
    /// The voter uses its local hot cache plus the small source/target delta.
    /// It does not download every cumulative Parquet segment.
    pub(crate) fn adopt_archive_retention_delta(
        &self,
        receipt: &crate::storage::archive::ArchiveReceipt,
        expected_raw_cursor: u64,
    ) -> Result<()> {
        let delta = receipt
            .manifest
            .retention_delta
            .as_ref()
            .context("archive retention checkpoint is missing its source delta")?;
        if receipt.manifest.raft_snapshot_index != expected_raw_cursor
            || receipt.manifest.retention_generation != delta.source_generation.saturating_add(1)
        {
            bail!("archive retention delta does not match its Raft cursor or generation");
        }
        let local_status = crate::storage::archive::committed_status(self.data_dir())?
            .context("archive retention delta requires its source receipt")?;
        if local_status.manifest_uri != delta.source_manifest_uri
            || local_status.manifest_sha256 != delta.source_manifest_sha256
            || local_status.retention_generation != delta.source_generation
        {
            bail!("archive retention delta source receipt changed");
        }
        let (prefix_events, prefix_digest, generation) =
            self.checkpoint_identity(expected_raw_cursor)?;
        if generation != delta.source_generation
            || prefix_events != delta.source_event_count
            || hex::encode(prefix_digest) != delta.source_event_content_sha256
        {
            bail!("archive retention delta source content disagrees with the voter");
        }

        self.recovery_required.store(true, Ordering::Release);
        let cutoff = DateTime::<Utc>::from_timestamp_nanos(delta.cutoff_unix_nano);
        self.storage
            .evict_expired_before(cutoff, expected_raw_cursor)?;
        let watermarks =
            crate::storage::archive::adopt_verified_archive_receipt(self.data_dir(), receipt)?;
        self.apply_expiration_head(
            cutoff,
            delta.source_event_count,
            receipt.manifest.event_count,
            decode_digest(&delta.source_event_content_sha256)?,
            decode_digest(&receipt.manifest.event_content_sha256)?,
            false,
        )?;
        crate::storage::archive::resume_local_blob_gc_batch(self, 128, 1_280_000)?;
        self.wal.compact_through(watermarks)?;
        self.recovery_required.store(false, Ordering::Release);
        Ok(())
    }

    /// Adopt newer durable archive coverage when retention did not change.
    /// A caught-up voter already has the same logical rows through Raft, so it
    /// only needs the small manifest receipt and can avoid a cumulative GCS
    /// restore on every lifecycle tick.
    pub(crate) fn adopt_archive_coverage(
        &self,
        receipt: &crate::storage::archive::ArchiveReceipt,
        expected_raw_cursor: u64,
    ) -> Result<()> {
        if receipt.manifest.raft_snapshot_index != expected_raw_cursor {
            bail!("verified archive coverage does not match its Raft cursor");
        }
        let (prefix_events, prefix_digest, _) = self.checkpoint_identity(expected_raw_cursor)?;
        if prefix_events != receipt.manifest.event_count
            || hex::encode(prefix_digest) != receipt.manifest.event_content_sha256
        {
            bail!("archive coverage content disagrees with the caught-up journal");
        }
        self.recovery_required.store(true, Ordering::Release);
        let mut state = self.state.write().expect("journal state lock poisoned");
        if state.last_cursor < expected_raw_cursor {
            bail!("Sift journal is behind archive coverage");
        }
        if receipt.manifest.retention_generation != state.retention_generation {
            bail!("archive coverage changed retention and requires full reconciliation");
        }
        let suffix_events = state.last_cursor.saturating_sub(expected_raw_cursor);
        let expected_events = receipt
            .manifest
            .event_count
            .checked_add(suffix_events)
            .context("archive coverage event count exhausted u64")?;
        if state.total_events != expected_events {
            bail!("archive coverage event count disagrees with the caught-up journal");
        }
        let watermarks =
            crate::storage::archive::adopt_verified_archive_receipt(self.data_dir(), receipt)?;
        state.retention_generation = receipt.manifest.retention_generation;
        crate::storage::JournalHead::new(state.last_cursor, state.total_events)
            .with_projection_generation(state.projection_generation)
            .with_retention_generation(state.retention_generation)
            .persist(self.data_dir())?;
        self.wal.compact_through(watermarks)?;
        self.recovery_required.store(false, Ordering::Release);
        Ok(())
    }
}
