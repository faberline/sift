//! Advancing the dedupe window: sealing, flushing and dropping generations.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::journal::domain::event_digest::{shard_for, xor_digest_in_place};
use crate::journal::domain::idempotency_window::{generation_for, oldest_generation};
use crate::journal::infrastructure::storage::dedupe_generation_store::{
    append_entries, generation_path, pending_entry_count, persist_meta, remove_generation_dir,
    seal_generation,
};
use crate::journal::infrastructure::storage::dedupe_index::{
    DedupeIndex, DedupeRecord, BACKGROUND_FLUSH_ENTRIES,
};

impl DedupeIndex {
    pub(crate) fn maintain_at(&self, now: DateTime<Utc>, force: bool) -> Result<usize> {
        let _maintenance = self
            .maintenance_gate
            .lock()
            .expect("dedupe maintenance lock poisoned");
        let current = generation_for(now);
        let oldest = oldest_generation(now);

        let expired = {
            let mut state = self.state.write().expect("dedupe state lock poisoned");
            let generations = state
                .generations
                .keys()
                .copied()
                .filter(|generation| *generation < oldest)
                .collect::<Vec<_>>();
            let mut expired = Vec::with_capacity(generations.len());
            for generation in generations {
                if let Some(index) = state.generations.remove(&generation) {
                    xor_digest_in_place(&mut state.content_digest, index.content_digest);
                    expired.push(generation);
                }
            }
            expired
        };
        for generation in &expired {
            remove_generation_dir(&generation_path(&self.root, *generation))?;
        }

        let total_pending = {
            let state = self.state.read().expect("dedupe state lock poisoned");
            pending_entry_count(&state)
        };
        let flush_current = force || total_pending >= BACKGROUND_FLUSH_ENTRIES;
        let generations = {
            let state = self.state.read().expect("dedupe state lock poisoned");
            state
                .generations
                .iter()
                .filter_map(|(generation, index)| {
                    (!index.pending.is_empty() && (*generation < current || flush_current))
                        .then_some(*generation)
                })
                .collect::<Vec<_>>()
        };

        let mut flushed = 0_usize;
        for generation in generations {
            let pending = {
                let state = self.state.read().expect("dedupe state lock poisoned");
                state
                    .generations
                    .get(&generation)
                    .map(|index| index.pending.clone())
                    .unwrap_or_default()
            };
            let mut shards = BTreeMap::<usize, Vec<DedupeRecord>>::new();
            for (digest, (cursor, acknowledged_at)) in &pending {
                shards.entry(shard_for(digest)).or_default().push((
                    *digest,
                    *cursor,
                    *acknowledged_at,
                ));
            }
            for (shard, records) in shards {
                let mut state = self.state.write().expect("dedupe state lock poisoned");
                let Some(index) = state.generations.get_mut(&generation) else {
                    continue;
                };
                if let Err(error) = append_entries(
                    &generation_path(&self.root, generation).join(format!("{shard:03x}.idx")),
                    &records,
                ) {
                    state.rebuild_required = true;
                    return Err(error).context("flush rebuildable dedupe shard projection");
                }
                for (digest, cursor, acknowledged_at) in records {
                    if index.pending.get(&digest) == Some(&(cursor, acknowledged_at)) {
                        index.pending.remove(&digest);
                        flushed = flushed.saturating_add(1);
                    }
                }
            }
        }

        let to_seal = {
            let mut state = self.state.write().expect("dedupe state lock poisoned");
            state
                .generations
                .iter_mut()
                .filter_map(|(generation, index)| {
                    if *generation < current && index.pending.is_empty() && !index.sealed {
                        index.receipt_writer.take();
                        Some(*generation)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        for generation in to_seal {
            if let Err(error) = seal_generation(&generation_path(&self.root, generation)) {
                self.state
                    .write()
                    .expect("dedupe state lock poisoned")
                    .rebuild_required = true;
                return Err(error).context("seal rebuildable dedupe generation");
            }
            let mut state = self.state.write().expect("dedupe state lock poisoned");
            if let Some(index) = state.generations.get_mut(&generation) {
                if index.pending.is_empty() {
                    index.sealed = true;
                }
            }
        }

        if !expired.is_empty() || flushed > 0 {
            let state = self.state.read().expect("dedupe state lock poisoned");
            if !state.rebuild_required {
                persist_meta(
                    &self.root,
                    state.indexed_through_cursor,
                    state.content_digest,
                )?;
            }
        }
        Ok(flushed)
    }

    pub(crate) fn maintain_applied(&self, force: bool) -> Result<usize> {
        let applied = self
            .state
            .read()
            .expect("dedupe state lock poisoned")
            .applied_time_unix_nano;
        let Some(applied) = applied else {
            return Ok(0);
        };
        self.maintain_at(DateTime::<Utc>::from_timestamp_nanos(applied), force)
    }
}
