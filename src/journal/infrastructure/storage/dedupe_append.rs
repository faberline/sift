//! Recording acknowledged events and receipts in the dedupe index.

use std::collections::BTreeSet;
use std::fs::{self};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::journal::domain::bloom_filter::bloom_insert_scalable;
use crate::journal::domain::dedupe_receipt::DedupeReceipt;
use crate::journal::domain::event_digest::{
    dedupe_record_digest, event_digest, shard_for, xor_digest_in_place,
};
use crate::journal::domain::idempotency_window::{
    acknowledged_generation, acknowledged_nanos, event_is_active_at, generation_for,
    GENERATION_SECONDS, IDEMPOTENCY_WINDOW_SECONDS,
};
use crate::journal::infrastructure::storage::dedupe_generation_store::{
    generation_path, pending_entry_count, persist_meta_with_policy, GenerationState,
};
use crate::journal::infrastructure::storage::dedupe_index::{
    DedupeIndex, GroupedDedupeRecords, MAX_PENDING_ENTRIES,
};
use crate::journal::infrastructure::storage::dedupe_shard_file::{
    encode_receipt_records, set_private_dir, set_private_file, RECEIPT_LOG_FILE,
};
use crate::shared_kernel::stored_event::StoredEvent;

impl DedupeIndex {
    pub fn append_batch(&self, events: &[StoredEvent]) -> Result<()> {
        self.append_batch_at(events, Utc::now())
    }

    /// Check the rebuildable receipt projection before the canonical WAL is
    /// mutated. This check does not fsync. The canonical signal WAL remains
    /// the only acknowledgement durability boundary.
    pub(crate) fn preflight_append_at(
        &self,
        now: DateTime<Utc>,
        incoming_entries: usize,
    ) -> Result<()> {
        let state = self.state.read().expect("dedupe state lock poisoned");
        if state.rebuild_required {
            bail!("dedupe index requires a rebuild before ingest can continue");
        }
        let pending = pending_entry_count(&state);
        if pending.saturating_add(incoming_entries) > MAX_PENDING_ENTRIES {
            bail!(
                "dedupe shard projection is behind ({pending} pending entries); retry after maintenance"
            );
        }
        drop(state);
        let root = fs::symlink_metadata(&self.root)
            .with_context(|| format!("inspect dedupe index root {}", self.root.display()))?;
        if root.file_type().is_symlink() || !root.is_dir() {
            bail!(
                "dedupe index root {} is not a real directory",
                self.root.display()
            );
        }
        let directory = generation_path(&self.root, generation_for(now));
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                bail!(
                    "dedupe generation {} is not a real directory",
                    directory.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&directory)
                    .with_context(|| format!("create dedupe generation {}", directory.display()))?;
                set_private_dir(&directory)?;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("inspect dedupe generation {}", directory.display()));
            }
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn append_batch_at(&self, events: &[StoredEvent], now: DateTime<Utc>) -> Result<()> {
        self.advance_window_at(now)?;
        let mut page = BTreeSet::new();
        let mut entries = GroupedDedupeRecords::new();
        for event in events {
            let generation = acknowledged_generation(event)?;
            if !event_is_active_at(event, now)? {
                continue;
            }
            let digest = event_digest(&event.event.project, &event.event.event_id);
            if !page.insert(digest)
                || self
                    .lookup_at(&event.event.project, &event.event.event_id, now)?
                    .is_some()
            {
                bail!(
                    "active idempotency window contains duplicate event_id {}",
                    event.event.event_id
                );
            }
            entries
                .entry(generation)
                .or_default()
                .entry(shard_for(&digest))
                .or_default()
                .push((digest, event.cursor, acknowledged_nanos(event)?));
        }
        let indexed_through_cursor = events
            .iter()
            .map(|event| event.cursor)
            .max()
            .unwrap_or_default();
        self.append_grouped_records(entries, indexed_through_cursor, now)
    }

    pub(crate) fn append_receipts_at(
        &self,
        receipts: &[DedupeReceipt],
        indexed_through_cursor: u64,
        now: DateTime<Utc>,
    ) -> Result<()> {
        self.advance_window_at(now)?;
        let cutoff_nanos = (now - chrono::Duration::seconds(IDEMPOTENCY_WINDOW_SECONDS))
            .timestamp_nanos_opt()
            .context("dedupe receipt cutoff is outside the nanosecond range")?;
        let mut page = BTreeSet::new();
        let mut entries = GroupedDedupeRecords::new();
        for receipt in receipts {
            if receipt.project.is_empty()
                || receipt.event_id.is_empty()
                || receipt.cursor == 0
                || receipt.acknowledged_at_unix_nano < cutoff_nanos
            {
                continue;
            }
            let digest = event_digest(&receipt.project, &receipt.event_id);
            if !page.insert(digest) {
                continue;
            }
            match self.lookup_digest_record_at(&digest, now)? {
                Some((cursor, acknowledged_at))
                    if cursor == receipt.cursor
                        && acknowledged_at == receipt.acknowledged_at_unix_nano =>
                {
                    continue;
                }
                Some((cursor, _)) => bail!(
                    "active idempotency receipt {} conflicts at cursors {cursor} and {}",
                    receipt.event_id,
                    receipt.cursor
                ),
                None => {}
            }
            let generation = receipt
                .acknowledged_at_unix_nano
                .div_euclid(1_000_000_000)
                .div_euclid(GENERATION_SECONDS);
            entries
                .entry(generation)
                .or_default()
                .entry(shard_for(&digest))
                .or_default()
                .push((digest, receipt.cursor, receipt.acknowledged_at_unix_nano));
        }
        if entries.is_empty() {
            return Ok(());
        }
        self.append_grouped_records(entries, indexed_through_cursor, now)
    }

    fn append_grouped_records(
        &self,
        entries: GroupedDedupeRecords,
        indexed_through_cursor: u64,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let current = generation_for(now);
        let mut state = self.state.write().expect("dedupe state lock poisoned");
        let mut batch_content_digest = [0_u8; 32];
        for (generation, shards) in entries {
            let directory = generation_path(&self.root, generation);
            fs::create_dir_all(&directory)?;
            set_private_dir(&directory)?;
            let index = state
                .generations
                .entry(generation)
                .or_insert_with(|| GenerationState::empty(generation < current));
            if index.receipt_writer.is_none() {
                index.receipt_writer = Some(storage_durable::FramedLogWriter::open(
                    directory.join(RECEIPT_LOG_FILE),
                    storage_durable::FsyncPolicy::Os,
                )?);
                set_private_file(&directory.join(RECEIPT_LOG_FILE))?;
            }
            let receipt_records = shards
                .values()
                .flat_map(|entries| entries.iter().copied())
                .collect::<Vec<_>>();
            let receipt_cursor = receipt_records
                .iter()
                .map(|(_, cursor, _)| *cursor)
                .max()
                .context("dedupe receipt batch has no records")?;
            let receipt_payload = encode_receipt_records(&receipt_records);
            index
                .receipt_writer
                .as_mut()
                .expect("receipt writer initialized")
                .append(receipt_cursor, &receipt_payload)
                .context("append one rebuildable dedupe receipt batch")?;
            index.sealed = false;
            for (shard, shard_entries) in shards {
                for entry @ (digest, cursor, acknowledged_at) in shard_entries {
                    if index
                        .pending
                        .insert(digest, (cursor, acknowledged_at))
                        .is_some()
                    {
                        bail!("dedupe receipt batch repeated one event digest");
                    }
                    bloom_insert_scalable(&mut index.blooms, &digest);
                    index.entry_count = index.entry_count.saturating_add(1);
                    index.newest_cursor = index.newest_cursor.max(cursor);
                    index.newest_acknowledged_at_unix_nano = Some(
                        index
                            .newest_acknowledged_at_unix_nano
                            .unwrap_or(i64::MIN)
                            .max(acknowledged_at),
                    );
                    let digest = dedupe_record_digest(generation, shard, &entry);
                    xor_digest_in_place(&mut index.content_digest, digest);
                    xor_digest_in_place(&mut batch_content_digest, digest);
                }
            }
        }
        xor_digest_in_place(&mut state.content_digest, batch_content_digest);
        state.indexed_through_cursor = state.indexed_through_cursor.max(indexed_through_cursor);
        state.applied_time_unix_nano = Some(
            state.applied_time_unix_nano.unwrap_or(i64::MIN).max(
                now.timestamp_nanos_opt()
                    .context("dedupe applied time is outside the nanosecond range")?,
            ),
        );
        persist_meta_with_policy(
            &self.root,
            state.indexed_through_cursor,
            state.content_digest,
            storage_durable::FsyncPolicy::Os,
        )?;
        Ok(())
    }
}
