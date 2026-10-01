//! Application operations used by durability and checkpoint adapters.

use crate::journal::domain::dedupe_store::DedupeStore;
use crate::journal::domain::event_store::RawEventStore;
use crate::shared_kernel::append_result::AppendResult;
use crate::shared_kernel::event::{EventEnvelope, SignalKind};
use crate::shared_kernel::event_query::EventQuery;
use crate::shared_kernel::stored_event::StoredEvent;

pub(crate) type SealedPrefix = (
    u64,
    Vec<(
        SignalKind,
        crate::shared_kernel::segment_manifest::SegmentManifest,
    )>,
);
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Arc;

pub(crate) trait JournalApi: Send + Sync {
    type Error;
    fn ensure_queryable(&self) -> Result<(), Self::Error>;
    fn set_retention_fenced(&self, fenced: bool);
    fn mark_recovery_required(&self);
    fn recovery_required(&self) -> bool;
    fn retention_generation(&self) -> u64;
    fn govern_event(&self, event: EventEnvelope) -> Result<EventEnvelope, Self::Error>;
    fn last_cursor(&self) -> u64;
    fn projection_generation(&self) -> u64;
    fn data_dir(&self) -> &Path;
    fn snapshot_bounds(&self) -> (u64, u64);
    fn resident_event_count(&self) -> usize;
    fn total_event_count(&self) -> u64;
    fn seal_archive_prefix(&self) -> Result<SealedPrefix, Self::Error>;
    fn compact_archived_wal(
        &self,
        watermarks: crate::shared_kernel::archive_watermarks::ArchiveWatermarks,
    ) -> Result<(), Self::Error>;
    fn scan_blob_references_page(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<(Vec<String>, u64, bool), Self::Error>;
    fn finalize_blob_candidates_with_index(
        &self,
        hashes: &[String],
        after: u64,
        limit: usize,
        mark_live: &mut dyn FnMut(&str) -> Result<(), Self::Error>,
        is_live: &mut dyn FnMut(&str) -> Result<bool, Self::Error>,
    ) -> Result<(u64, usize, bool), Self::Error>;
    fn snapshot_bytes(&self) -> Result<Vec<u8>, Self::Error>;
    fn restore_snapshot_bytes(&self, bytes: &[u8]) -> Result<(), Self::Error>;
    fn append(&self, event: EventEnvelope) -> Result<AppendResult, Self::Error>;
    fn append_durable_batch_at(
        &self,
        events: Vec<EventEnvelope>,
        acknowledged_at: DateTime<Utc>,
    ) -> Result<Vec<AppendResult>, Self::Error>;
    fn result_for_at(
        &self,
        project: &str,
        event_id: &str,
        decision_time: DateTime<Utc>,
    ) -> Result<Option<AppendResult>, Self::Error>;
    fn query(&self, query: EventQuery) -> Result<Vec<StoredEvent>, Self::Error>;
    fn query_unchecked(&self, query: EventQuery) -> Result<Vec<StoredEvent>, Self::Error>;
    fn query_projection_events(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<Vec<StoredEvent>, Self::Error>;
    fn replay(&self, after: u64, limit: usize) -> Result<Vec<StoredEvent>, Self::Error>;
    fn adopt_archive_checkpoint(
        &self,
        restored: &dyn JournalApi<Error = Self::Error>,
        receipt: &crate::shared_kernel::archive_receipts::ArchiveReceipt,
        expected_raw_cursor: u64,
    ) -> Result<(), Self::Error>;
    fn adopt_archive_retention_delta(
        &self,
        receipt: &crate::shared_kernel::archive_receipts::ArchiveReceipt,
        expected_raw_cursor: u64,
    ) -> Result<(), Self::Error>;
    fn adopt_archive_coverage(
        &self,
        receipt: &crate::shared_kernel::archive_receipts::ArchiveReceipt,
        expected_raw_cursor: u64,
    ) -> Result<(), Self::Error>;
    fn restore_stored_page(&self, events: Vec<StoredEvent>) -> Result<(), Self::Error>;
    fn restore_archive_dedupe_page(&self, events: &[StoredEvent]) -> Result<(), Self::Error>;
    fn restore_archive_receipts(
        &self,
        receipts: &[crate::shared_kernel::dedupe_receipt::DedupeReceipt],
        indexed_through_cursor: u64,
    ) -> Result<(), Self::Error>;
    fn set_restored_archive_head(
        &self,
        manifest: &crate::shared_kernel::archive_manifest::ArchiveManifest,
    ) -> Result<(), Self::Error>;
    fn evict_resident_before(&self, cutoff: DateTime<Utc>) -> Result<usize, Self::Error>;
    fn apply_expiration_head(
        &self,
        cutoff: DateTime<Utc>,
        archived_prefix_events: u64,
        retained_prefix_events: u64,
        archived_prefix_digest: [u8; 32],
        retained_prefix_digest: [u8; 32],
        repair_dedupe: bool,
    ) -> Result<(), Self::Error>;
    fn checkpoint_identity(&self, raw_cursor: u64) -> Result<(u64, [u8; 32], u64), Self::Error>;
    fn maintain_dedupe_at(&self, _now: DateTime<Utc>, force: bool) -> Result<usize, Self::Error>;
    fn event_store(&self) -> Arc<dyn RawEventStore<Error = Self::Error>>;
    fn storage(&self) -> &dyn RawEventStore<Error = Self::Error>;
    fn dedupe(&self) -> &dyn DedupeStore<Error = Self::Error>;
    fn archive_read(
        &self,
    ) -> &dyn crate::journal::domain::archive_access::ArchiveRead<Error = Self::Error>;
    fn archive_maintenance(
        &self,
    ) -> &dyn crate::journal::domain::archive_access::ArchiveMaintenance<Error = Self::Error>;
    fn open_staging(
        &self,
        root: &Path,
    ) -> Result<Arc<dyn JournalApi<Error = Self::Error>>, Self::Error>;
    fn metrics_snapshot(&self) -> [u64; 4];
    fn projection_read_session(
        self: Arc<Self>,
        after: u64,
    ) -> Result<
        Box<dyn crate::journal::domain::projection_reader::ProjectionReader<Error = Self::Error>>,
        Self::Error,
    >;
}
