//! Restoring stored pages, archive dedupe receipts and the archive head.

use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use chrono::Utc;

use crate::journal::domain::recent_cursor::recent_cursor_at;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::shared_kernel::stored_event::StoredEvent;

impl DurableJournal {
    /// Restore one globally ordered page without creating a second full-copy
    /// JSON snapshot. Cold archive restore calls this repeatedly, so resident
    /// memory stays bounded by the journal cache plus one recovery page.
    pub(crate) fn restore_stored_page(&self, events: Vec<StoredEvent>) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let restored_events = events.len() as u64;
        let restore_time = Utc::now();
        let mut state = self.state.write().expect("journal state lock poisoned");
        let mut previous_cursor = state.last_cursor;
        let mut page_ids = HashMap::with_capacity(events.len());
        for event in &events {
            event.event.validate()?;
            if event.cursor <= previous_cursor {
                bail!(
                    "restored journal cursor {} is not strictly after cursor {previous_cursor}",
                    event.cursor,
                );
            }
            previous_cursor = event.cursor;
            if !self.dedupe.covers(event, restore_time)? {
                continue;
            }
            if let Some(previous) = page_ids
                .insert(
                    (event.event.project.clone(), event.event.event_id.clone()),
                    event.cursor,
                )
                .or_else(|| {
                    recent_cursor_at(
                        &state.recent_cursors_by_event_id,
                        &event.event.project,
                        &event.event.event_id,
                        restore_time,
                    )
                })
                .or(self.dedupe.lookup_at(
                    &event.event.project,
                    &event.event.event_id,
                    restore_time,
                )?)
            {
                bail!(
                    "restored journal contains duplicate event_id {} at cursors {previous} and {}",
                    event.event.event_id,
                    event.cursor
                );
            }
        }

        // A signal WAL frame must contain one contiguous same-signal run.
        // Preserve global cursor order while still avoiding one fsync per item.
        let mut start = 0;
        while start < events.len() {
            let signal = events[start].event.signal;
            let mut end = start + 1;
            while end < events.len()
                && events[end].event.signal == signal
                && events[end - 1].cursor.checked_add(1) == Some(events[end].cursor)
            {
                end += 1;
            }
            self.wal
                .append_batch(&events[start..end])
                .context("restore ordered page into signal WAL")?;
            self.fsyncs.incr();
            start = end;
        }
        self.storage
            .append_batch(&events)
            .context("restore ordered page into signal segments")?;
        self.dedupe
            .append_batch_at(&events, restore_time)
            .context("restore ordered page into dedupe index")?;
        self.dedupe.maintain_at(restore_time, false)?;
        for event in events {
            Self::push_resident(&mut state, event, self.resident_limit)?;
        }
        crate::storage::JournalHead::new(state.last_cursor, state.total_events)
            .with_projection_generation(state.projection_generation)
            .with_retention_generation(state.retention_generation)
            .persist(self.data_dir())
            .context("persist restored journal head")?;
        self.accepted.add(restored_events);
        Ok(())
    }

    pub(crate) fn restore_archive_dedupe_page(&self, events: &[StoredEvent]) -> Result<()> {
        self.dedupe
            .append_batch(events)
            .context("restore cold archive IDs into the dedupe index")?;
        self.dedupe.maintain_at(Utc::now(), false)?;
        Ok(())
    }

    pub(crate) fn restore_archive_receipts(
        &self,
        receipts: &[crate::storage::DedupeReceipt],
        indexed_through_cursor: u64,
    ) -> Result<()> {
        self.dedupe
            .append_receipts_at(receipts, indexed_through_cursor, Utc::now())
            .context("restore independent archive dedupe receipts")?;
        self.dedupe.maintain_at(Utc::now(), false)?;
        Ok(())
    }

    pub(crate) fn set_restored_archive_head(
        &self,
        manifest: &crate::storage::archive::ArchiveManifest,
    ) -> Result<()> {
        self.dedupe
            .mark_rebuilt_through(manifest.raft_snapshot_index)?;
        let dedupe = self.dedupe.stats()?;
        if dedupe.newest_cursor > manifest.raft_snapshot_index
            || dedupe.window_seconds != crate::storage::IDEMPOTENCY_WINDOW_SECONDS as u64
        {
            bail!("restored archive dedupe index disagrees with its manifest");
        }
        let mut state = self.state.write().expect("journal state lock poisoned");
        if state.last_cursor > manifest.raft_snapshot_index
            || state.total_events > manifest.event_count
        {
            bail!("restored hot set exceeds its archive manifest");
        }
        let event_content_digest: [u8; 32] = hex::decode(&manifest.event_content_sha256)
            .context("decode restored archive event content digest")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("archive event content digest must be 32 bytes"))?;
        let newly_counted = manifest.event_count.saturating_sub(state.total_events);
        state.last_cursor = manifest.raft_snapshot_index;
        state.total_events = manifest.event_count;
        state.retention_generation = manifest.retention_generation;
        state.event_content_digest = event_content_digest;
        state.projection_generation = state.projection_generation.saturating_add(1);
        crate::storage::JournalHead::new(state.last_cursor, state.total_events)
            .with_projection_generation(state.projection_generation)
            .with_retention_generation(state.retention_generation)
            .persist(self.data_dir())?;
        self.accepted.add(newly_counted);
        Ok(())
    }
}
