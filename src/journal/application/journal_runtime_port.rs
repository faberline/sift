//! The live application runtime implements the checkpoint-facing port.

use crate::journal::application::journal_runtime::JournalRuntime;
use crate::journal::domain::journal_api::JournalApi;
use crate::shared_kernel::append_result::AppendResult;
use crate::shared_kernel::event::{EventEnvelope, SignalKind};
use crate::shared_kernel::event_query::EventQuery;
use crate::shared_kernel::stored_event::StoredEvent;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Arc;

impl JournalApi for JournalRuntime {
    type Error = anyhow::Error;
    fn ensure_queryable(&self) -> Result<()> {
        JournalRuntime::ensure_queryable(self)
    }
    fn set_retention_fenced(&self, fenced: bool) {
        JournalRuntime::set_retention_fenced(self, fenced)
    }
    fn mark_recovery_required(&self) {
        JournalRuntime::mark_recovery_required(self)
    }
    fn recovery_required(&self) -> bool {
        JournalRuntime::recovery_required(self)
    }
    fn retention_generation(&self) -> u64 {
        JournalRuntime::retention_generation(self)
    }
    fn govern_event(&self, event: EventEnvelope) -> Result<EventEnvelope> {
        JournalRuntime::govern_event(self, event)
    }
    fn last_cursor(&self) -> u64 {
        JournalRuntime::last_cursor(self)
    }
    fn projection_generation(&self) -> u64 {
        JournalRuntime::projection_generation(self)
    }
    fn data_dir(&self) -> &Path {
        JournalRuntime::data_dir(self)
    }
    fn snapshot_bounds(&self) -> (u64, u64) {
        JournalRuntime::snapshot_bounds(self)
    }
    fn resident_event_count(&self) -> usize {
        JournalRuntime::resident_event_count(self)
    }
    fn total_event_count(&self) -> u64 {
        JournalRuntime::total_event_count(self)
    }
    fn seal_archive_prefix(
        &self,
    ) -> Result<(
        u64,
        Vec<(
            SignalKind,
            crate::shared_kernel::segment_manifest::SegmentManifest,
        )>,
    )> {
        JournalRuntime::seal_archive_prefix(self)
    }
    fn compact_archived_wal(
        &self,
        watermarks: crate::shared_kernel::archive_watermarks::ArchiveWatermarks,
    ) -> Result<()> {
        JournalRuntime::compact_archived_wal(self, watermarks)
    }
    fn scan_blob_references_page(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<(Vec<String>, u64, bool)> {
        JournalRuntime::scan_blob_references_page(self, after, limit)
    }
    fn finalize_blob_candidates_with_index(
        &self,
        hashes: &[String],
        after: u64,
        limit: usize,
        mark_live: &mut dyn FnMut(&str) -> Result<()>,
        is_live: &mut dyn FnMut(&str) -> Result<bool>,
    ) -> Result<(u64, usize, bool)> {
        JournalRuntime::finalize_blob_candidates_with_index(
            self, hashes, after, limit, mark_live, is_live,
        )
    }
    fn snapshot_bytes(&self) -> Result<Vec<u8>> {
        JournalRuntime::snapshot_bytes(self)
    }
    fn restore_snapshot_bytes(&self, bytes: &[u8]) -> Result<()> {
        JournalRuntime::restore_snapshot_bytes(self, bytes)
    }
    fn append(&self, event: EventEnvelope) -> Result<AppendResult> {
        JournalRuntime::append(self, event)
    }
    fn append_durable_batch_at(
        &self,
        events: Vec<EventEnvelope>,
        acknowledged_at: DateTime<Utc>,
    ) -> Result<Vec<AppendResult>> {
        JournalRuntime::append_durable_batch_at(self, events, acknowledged_at)
    }
    fn result_for_at(
        &self,
        project: &str,
        event_id: &str,
        decision_time: DateTime<Utc>,
    ) -> Result<Option<AppendResult>> {
        JournalRuntime::result_for_at(self, project, event_id, decision_time)
    }
    fn query(&self, query: EventQuery) -> Result<Vec<StoredEvent>> {
        JournalRuntime::query(self, query)
    }
    fn query_unchecked(&self, query: EventQuery) -> Result<Vec<StoredEvent>> {
        JournalRuntime::query_unchecked(self, query)
    }
    fn query_projection_events(&self, after: u64, limit: usize) -> Result<Vec<StoredEvent>> {
        JournalRuntime::query_projection_events(self, after, limit)
    }
    fn replay(&self, after: u64, limit: usize) -> Result<Vec<StoredEvent>> {
        JournalRuntime::replay(self, after, limit)
    }
    fn adopt_archive_checkpoint(
        &self,
        restored: &dyn JournalApi<Error = Self::Error>,
        receipt: &crate::shared_kernel::archive_receipts::ArchiveReceipt,
        expected_raw_cursor: u64,
    ) -> Result<()> {
        JournalRuntime::adopt_archive_checkpoint(self, restored, receipt, expected_raw_cursor)
    }
    fn adopt_archive_retention_delta(
        &self,
        receipt: &crate::shared_kernel::archive_receipts::ArchiveReceipt,
        expected_raw_cursor: u64,
    ) -> Result<()> {
        JournalRuntime::adopt_archive_retention_delta(self, receipt, expected_raw_cursor)
    }
    fn adopt_archive_coverage(
        &self,
        receipt: &crate::shared_kernel::archive_receipts::ArchiveReceipt,
        expected_raw_cursor: u64,
    ) -> Result<()> {
        JournalRuntime::adopt_archive_coverage(self, receipt, expected_raw_cursor)
    }
    fn restore_stored_page(&self, events: Vec<StoredEvent>) -> Result<()> {
        JournalRuntime::restore_stored_page(self, events)
    }
    fn restore_archive_dedupe_page(&self, events: &[StoredEvent]) -> Result<()> {
        JournalRuntime::restore_archive_dedupe_page(self, events)
    }
    fn restore_archive_receipts(
        &self,
        receipts: &[crate::shared_kernel::dedupe_receipt::DedupeReceipt],
        indexed_through_cursor: u64,
    ) -> Result<()> {
        JournalRuntime::restore_archive_receipts(self, receipts, indexed_through_cursor)
    }
    fn set_restored_archive_head(
        &self,
        manifest: &crate::shared_kernel::archive_manifest::ArchiveManifest,
    ) -> Result<()> {
        JournalRuntime::set_restored_archive_head(self, manifest)
    }
    fn evict_resident_before(&self, cutoff: DateTime<Utc>) -> Result<usize> {
        JournalRuntime::evict_resident_before(self, cutoff)
    }
    fn apply_expiration_head(
        &self,
        cutoff: DateTime<Utc>,
        archived_prefix_events: u64,
        retained_prefix_events: u64,
        archived_prefix_digest: [u8; 32],
        retained_prefix_digest: [u8; 32],
        repair_dedupe: bool,
    ) -> Result<()> {
        JournalRuntime::apply_expiration_head(
            self,
            cutoff,
            archived_prefix_events,
            retained_prefix_events,
            archived_prefix_digest,
            retained_prefix_digest,
            repair_dedupe,
        )
    }
    fn checkpoint_identity(&self, raw_cursor: u64) -> Result<(u64, [u8; 32], u64)> {
        JournalRuntime::checkpoint_identity(self, raw_cursor)
    }
    fn maintain_dedupe_at(&self, _now: DateTime<Utc>, force: bool) -> Result<usize> {
        JournalRuntime::maintain_dedupe_at(self, _now, force)
    }
    fn event_store(
        &self,
    ) -> Arc<dyn crate::journal::domain::event_store::RawEventStore<Error = Self::Error>> {
        self.storage.clone()
    }
    fn storage(
        &self,
    ) -> &dyn crate::journal::domain::event_store::RawEventStore<Error = Self::Error> {
        self.storage.as_ref()
    }
    fn dedupe(
        &self,
    ) -> &dyn crate::journal::domain::dedupe_store::DedupeStore<Error = Self::Error> {
        self.dedupe.as_ref()
    }
    fn archive_read(
        &self,
    ) -> &dyn crate::journal::domain::archive_access::ArchiveRead<Error = Self::Error> {
        self.archive_read.as_ref()
    }
    fn archive_maintenance(
        &self,
    ) -> &dyn crate::journal::domain::archive_access::ArchiveMaintenance<Error = Self::Error> {
        self.archive_maintenance.as_ref()
    }
    fn open_staging(&self, root: &Path) -> Result<Arc<dyn JournalApi<Error = Self::Error>>> {
        self.staging.open(root)
    }
    fn metrics_snapshot(&self) -> [u64; 4] {
        [
            self.accepted.get(),
            self.duplicates.get(),
            self.fsyncs.get(),
            self.resident_event_count() as u64,
        ]
    }
    fn projection_read_session(
        self: Arc<Self>,
        after: u64,
    ) -> Result<
        Box<dyn crate::journal::domain::projection_reader::ProjectionReader<Error = Self::Error>>,
    > {
        Ok(Box::new(JournalRuntime::projection_read_session(
            self, after,
        )?))
    }
}
