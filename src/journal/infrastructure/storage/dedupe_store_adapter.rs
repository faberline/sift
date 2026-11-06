//! The dedupe port preserves the index's own lock and fsync order.

use crate::journal::domain::dedupe_store::{DedupeCopySource, DedupeStore};
use crate::journal::infrastructure::storage::dedupe_index::{DedupeIndex, DedupeState};
use crate::shared_kernel::dedupe_receipt::DedupeReceipt;
use crate::shared_kernel::dedupe_stats::DedupeStats;
use crate::shared_kernel::stored_event::StoredEvent;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::RwLockReadGuard;

impl DedupeStore for DedupeIndex {
    type Error = anyhow::Error;
    fn lookup_at(&self, project: &str, event_id: &str, now: DateTime<Utc>) -> Result<Option<u64>> {
        DedupeIndex::lookup_at(self, project, event_id, now)
    }
    fn lookup_record_at(
        &self,
        project: &str,
        event_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<(u64, i64)>> {
        DedupeIndex::lookup_record_at(self, project, event_id, now)
    }
    fn covers(&self, event: &StoredEvent, now: DateTime<Utc>) -> Result<bool> {
        DedupeIndex::covers(self, event, now)
    }
    fn stats(&self) -> Result<DedupeStats> {
        DedupeIndex::stats(self)
    }
    fn stats_at(&self, now: DateTime<Utc>) -> Result<DedupeStats> {
        DedupeIndex::stats_at(self, now)
    }
    fn reset(&self) -> Result<()> {
        DedupeIndex::reset(self)
    }
    fn mark_rebuilt_through(&self, cursor: u64) -> Result<()> {
        DedupeIndex::mark_rebuilt_through(self, cursor)
    }
    fn advance_window_at(&self, _now: DateTime<Utc>) -> Result<()> {
        DedupeIndex::advance_window_at(self, _now)
    }
    fn append_batch(&self, events: &[StoredEvent]) -> Result<()> {
        DedupeIndex::append_batch(self, events)
    }
    fn preflight_append_at(&self, now: DateTime<Utc>, incoming_entries: usize) -> Result<()> {
        DedupeIndex::preflight_append_at(self, now, incoming_entries)
    }
    fn append_batch_at(&self, events: &[StoredEvent], now: DateTime<Utc>) -> Result<()> {
        DedupeIndex::append_batch_at(self, events, now)
    }
    fn append_receipts_at(
        &self,
        receipts: &[DedupeReceipt],
        indexed_through_cursor: u64,
        now: DateTime<Utc>,
    ) -> Result<()> {
        DedupeIndex::append_receipts_at(self, receipts, indexed_through_cursor, now)
    }
    fn maintain_at(&self, now: DateTime<Utc>, force: bool) -> Result<usize> {
        DedupeIndex::maintain_at(self, now, force)
    }
    fn maintain_applied(&self, force: bool) -> Result<usize> {
        DedupeIndex::maintain_applied(self, force)
    }
    fn preflight_rebuild(&self) -> Result<()> {
        DedupeIndex::preflight_rebuild(self)
    }
    fn copy_source(&self) -> Box<dyn DedupeCopySource + '_> {
        let guard = self.state.read().expect("source dedupe lock poisoned");
        Box::new(LockedSource {
            _guard: guard,
            root: &self.root,
        })
    }
    fn replace_from(&self, source: &dyn DedupeStore<Error = Self::Error>) -> Result<DedupeStats> {
        let source = source.copy_source();
        self.replace_from_root(source.root())
    }
}
struct LockedSource<'a> {
    _guard: RwLockReadGuard<'a, DedupeState>,
    root: &'a Path,
}
impl DedupeCopySource for LockedSource<'_> {
    fn root(&self) -> &Path {
        self.root
    }
}
