//! The idempotency index and its locked replacement source.

use crate::shared_kernel::dedupe_receipt::DedupeReceipt;
use crate::shared_kernel::dedupe_stats::DedupeStats;
use crate::shared_kernel::stored_event::StoredEvent;
use chrono::{DateTime, Utc};
use std::path::Path;

pub(crate) trait DedupeCopySource {
    fn root(&self) -> &Path;
}
pub(crate) trait DedupeStore: Send + Sync {
    type Error;
    fn lookup_at(
        &self,
        project: &str,
        event_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<u64>, Self::Error>;
    fn lookup_record_at(
        &self,
        project: &str,
        event_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<(u64, i64)>, Self::Error>;
    fn covers(&self, event: &StoredEvent, now: DateTime<Utc>) -> Result<bool, Self::Error>;
    fn stats(&self) -> Result<DedupeStats, Self::Error>;
    fn stats_at(&self, now: DateTime<Utc>) -> Result<DedupeStats, Self::Error>;
    fn reset(&self) -> Result<(), Self::Error>;
    fn mark_rebuilt_through(&self, cursor: u64) -> Result<(), Self::Error>;
    fn advance_window_at(&self, _now: DateTime<Utc>) -> Result<(), Self::Error>;
    fn append_batch(&self, events: &[StoredEvent]) -> Result<(), Self::Error>;
    fn preflight_append_at(
        &self,
        now: DateTime<Utc>,
        incoming_entries: usize,
    ) -> Result<(), Self::Error>;
    fn append_batch_at(
        &self,
        events: &[StoredEvent],
        now: DateTime<Utc>,
    ) -> Result<(), Self::Error>;
    fn append_receipts_at(
        &self,
        receipts: &[DedupeReceipt],
        indexed_through_cursor: u64,
        now: DateTime<Utc>,
    ) -> Result<(), Self::Error>;
    fn maintain_at(&self, now: DateTime<Utc>, force: bool) -> Result<usize, Self::Error>;
    fn maintain_applied(&self, force: bool) -> Result<usize, Self::Error>;
    fn preflight_rebuild(&self) -> Result<(), Self::Error>;
    fn copy_source(&self) -> Box<dyn DedupeCopySource + '_>;
    fn replace_from(
        &self,
        source: &dyn DedupeStore<Error = Self::Error>,
    ) -> Result<DedupeStats, Self::Error>;
}

impl<T: DedupeStore + ?Sized> DedupeStore for std::sync::Arc<T> {
    type Error = T::Error;
    fn lookup_at(
        &self,
        project: &str,
        event_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<u64>, Self::Error> {
        self.as_ref().lookup_at(project, event_id, now)
    }
    fn lookup_record_at(
        &self,
        project: &str,
        event_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<(u64, i64)>, Self::Error> {
        self.as_ref().lookup_record_at(project, event_id, now)
    }
    fn covers(&self, event: &StoredEvent, now: DateTime<Utc>) -> Result<bool, Self::Error> {
        self.as_ref().covers(event, now)
    }
    fn stats(&self) -> Result<DedupeStats, Self::Error> {
        self.as_ref().stats()
    }
    fn stats_at(&self, now: DateTime<Utc>) -> Result<DedupeStats, Self::Error> {
        self.as_ref().stats_at(now)
    }
    fn reset(&self) -> Result<(), Self::Error> {
        self.as_ref().reset()
    }
    fn mark_rebuilt_through(&self, cursor: u64) -> Result<(), Self::Error> {
        self.as_ref().mark_rebuilt_through(cursor)
    }
    fn advance_window_at(&self, _now: DateTime<Utc>) -> Result<(), Self::Error> {
        self.as_ref().advance_window_at(_now)
    }
    fn append_batch(&self, events: &[StoredEvent]) -> Result<(), Self::Error> {
        self.as_ref().append_batch(events)
    }
    fn preflight_append_at(
        &self,
        now: DateTime<Utc>,
        incoming_entries: usize,
    ) -> Result<(), Self::Error> {
        self.as_ref().preflight_append_at(now, incoming_entries)
    }
    fn append_batch_at(
        &self,
        events: &[StoredEvent],
        now: DateTime<Utc>,
    ) -> Result<(), Self::Error> {
        self.as_ref().append_batch_at(events, now)
    }
    fn append_receipts_at(
        &self,
        receipts: &[DedupeReceipt],
        indexed_through_cursor: u64,
        now: DateTime<Utc>,
    ) -> Result<(), Self::Error> {
        self.as_ref()
            .append_receipts_at(receipts, indexed_through_cursor, now)
    }
    fn maintain_at(&self, now: DateTime<Utc>, force: bool) -> Result<usize, Self::Error> {
        self.as_ref().maintain_at(now, force)
    }
    fn maintain_applied(&self, force: bool) -> Result<usize, Self::Error> {
        self.as_ref().maintain_applied(force)
    }
    fn preflight_rebuild(&self) -> Result<(), Self::Error> {
        self.as_ref().preflight_rebuild()
    }
    fn copy_source(&self) -> Box<dyn DedupeCopySource + '_> {
        self.as_ref().copy_source()
    }
    fn replace_from(
        &self,
        source: &dyn DedupeStore<Error = Self::Error>,
    ) -> Result<DedupeStats, Self::Error> {
        self.as_ref().replace_from(source)
    }
}
