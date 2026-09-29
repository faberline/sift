//! Bounded, rebuildable event-id index.
//!
//! Sift guarantees exact idempotency for six hours after acknowledgement.
//! The canonical WAL and committed segments own telemetry durability. This
//! index keeps only the active acknowledgement generations and can be rebuilt.

use std::collections::BTreeMap;
use std::fs::{self};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::journal::domain::bloom_filter::bloom_contains;
use crate::journal::domain::dedupe_stats::DedupeStats;
use crate::journal::domain::event_digest::event_digest;
use crate::journal::domain::idempotency_window::{
    event_is_active_at, generation_for, oldest_generation, IDEMPOTENCY_WINDOW_SECONDS,
};
use crate::journal::infrastructure::storage::dedupe_generation_store::{
    clear_root, generation_path, load_state, pending_entry_count, persist_meta, shard_path,
    stats_from_state, GenerationState, META_FILE,
};
use crate::journal::infrastructure::storage::dedupe_replace::reconcile_replace;
use crate::journal::infrastructure::storage::dedupe_shard_file::{lookup_file, set_private_dir};
use crate::shared_kernel::stored_event::StoredEvent;

pub(super) const BACKGROUND_FLUSH_ENTRIES: usize = 100_000;

pub(super) const MAX_PENDING_ENTRIES: usize = BACKGROUND_FLUSH_ENTRIES * 2;

pub(super) type DedupeRecord = ([u8; 32], u64, i64);

pub(super) type GroupedDedupeRecords = BTreeMap<i64, BTreeMap<usize, Vec<DedupeRecord>>>;

pub struct DedupeIndex {
    pub(super) root: PathBuf,
    pub(super) state: RwLock<DedupeState>,
    pub(super) maintenance_gate: Mutex<()>,
}

#[derive(Default)]
pub(super) struct DedupeState {
    pub(super) generations: BTreeMap<i64, GenerationState>,
    pub(super) indexed_through_cursor: u64,
    pub(super) content_digest: [u8; 32],
    pub(super) rebuild_required: bool,
    pub(super) applied_time_unix_nano: Option<i64>,
}

impl DedupeIndex {
    pub fn open(root: impl AsRef<Path>) -> Result<(Self, DedupeStats)> {
        Self::open_at(root, Utc::now())
    }

    #[doc(hidden)]
    pub fn open_at(root: impl AsRef<Path>, now: DateTime<Utc>) -> Result<(Self, DedupeStats)> {
        let root = root.as_ref().join("indexes").join("dedupe");
        reconcile_replace(&root, now)?;
        fs::create_dir_all(&root)
            .with_context(|| format!("create dedupe index root {}", root.display()))?;
        set_private_dir(&root)?;
        let state = match load_state(&root, generation_for(now)) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(%error, "rebuilding damaged Sift dedupe index");
                clear_root(&root)?;
                DedupeState {
                    rebuild_required: true,
                    ..DedupeState::default()
                }
            }
        };
        let index = Self {
            root,
            state: RwLock::new(state),
            maintenance_gate: Mutex::new(()),
        };
        // Do not prune from the process wall clock while opening. A voter can
        // restart hours after it last applied Raft. It must first replay every
        // missed command in log order, using the decision time carried by that
        // command. `append_batch_at` advances the window after each apply.
        // Pruning here could remove the row needed to reproduce the leader's
        // earlier duplicate decision.
        let stats = index.stats_at(now)?;
        Ok((index, stats))
    }

    pub fn lookup(&self, project: &str, event_id: &str) -> Result<Option<u64>> {
        self.lookup_at(project, event_id, Utc::now())
    }

    #[doc(hidden)]
    pub fn lookup_at(
        &self,
        project: &str,
        event_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<u64>> {
        Ok(self
            .lookup_record_at(project, event_id, now)?
            .map(|(cursor, _)| cursor))
    }

    pub(crate) fn lookup_record_at(
        &self,
        project: &str,
        event_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<(u64, i64)>> {
        let digest = event_digest(project, event_id);
        self.lookup_digest_record_at(&digest, now)
    }

    pub(super) fn lookup_digest_record_at(
        &self,
        digest: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<Option<(u64, i64)>> {
        let oldest = oldest_generation(now);
        let cutoff_nanos = (now - chrono::Duration::seconds(IDEMPOTENCY_WINDOW_SECONDS))
            .timestamp_nanos_opt()
            .context("dedupe lookup time is outside the nanosecond range")?;
        let state = self.state.read().expect("dedupe state lock poisoned");
        for (generation, index) in state.generations.range(oldest..).rev() {
            if !index
                .blooms
                .iter()
                .any(|layer| bloom_contains(&layer.bits, digest))
            {
                continue;
            }
            if let Some((cursor, acknowledged_at)) = index.pending.get(digest).copied() {
                if acknowledged_at >= cutoff_nanos {
                    return Ok(Some((cursor, acknowledged_at)));
                }
            }
            let path = shard_path(&self.root, *generation, digest);
            if let Some((cursor, acknowledged_at)) = lookup_file(&path, digest, index.sealed)? {
                if acknowledged_at >= cutoff_nanos {
                    return Ok(Some((cursor, acknowledged_at)));
                }
            }
        }
        Ok(None)
    }

    pub(crate) fn covers(&self, event: &StoredEvent, now: DateTime<Utc>) -> Result<bool> {
        event_is_active_at(event, now)
    }

    pub(crate) fn stats(&self) -> Result<DedupeStats> {
        self.stats_at(Utc::now())
    }

    #[doc(hidden)]
    pub fn stats_at(&self, now: DateTime<Utc>) -> Result<DedupeStats> {
        let oldest = oldest_generation(now);
        let state = self.state.read().expect("dedupe state lock poisoned");
        Ok(stats_from_state(&state, oldest))
    }

    pub fn reset(&self) -> Result<()> {
        let mut state = self.state.write().expect("dedupe state lock poisoned");
        clear_root(&self.root)?;
        *state = DedupeState {
            rebuild_required: true,
            ..DedupeState::default()
        };
        Ok(())
    }

    pub(crate) fn mark_rebuilt_through(&self, cursor: u64) -> Result<()> {
        let mut state = self.state.write().expect("dedupe state lock poisoned");
        state.indexed_through_cursor = cursor;
        state.rebuild_required = false;
        persist_meta(&self.root, cursor, state.content_digest)
    }

    #[doc(hidden)]
    pub fn advance_window_at(&self, _now: DateTime<Utc>) -> Result<()> {
        // Expiration and shard sealing can touch thousands of files. The
        // projection worker owns that rebuildable work. The ingest path only
        // appends one receipt frame after the canonical signal WAL is durable.
        Ok(())
    }

    #[doc(hidden)]
    pub fn maintain_for_applied_time_for_diagnostics(&self, force: bool) -> Result<usize> {
        self.maintain_applied(force)
    }

    #[doc(hidden)]
    pub fn flush_pending_at_for_diagnostics(&self, now: DateTime<Utc>) -> Result<usize> {
        self.maintain_at(now, true)
    }

    #[doc(hidden)]
    pub fn pending_entry_count_for_diagnostics(&self) -> usize {
        pending_entry_count(&self.state.read().expect("dedupe state lock poisoned"))
    }

    #[doc(hidden)]
    pub fn max_pending_entries_for_diagnostics(&self) -> usize {
        MAX_PENDING_ENTRIES
    }

    #[doc(hidden)]
    pub fn disk_bytes(&self) -> Result<u64> {
        let state = self.state.read().expect("dedupe state lock poisoned");
        let mut bytes = 0_u64;
        for generation in state.generations.keys() {
            for entry in fs::read_dir(generation_path(&self.root, *generation))? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    bytes = bytes.saturating_add(entry.metadata()?.len());
                }
            }
        }
        let meta = self.root.join(META_FILE);
        if meta.exists() {
            bytes = bytes.saturating_add(fs::metadata(meta)?.len());
        }
        Ok(bytes)
    }

    #[doc(hidden)]
    pub fn active_generation_count(&self) -> usize {
        self.state
            .read()
            .expect("dedupe state lock poisoned")
            .generations
            .len()
    }
}
