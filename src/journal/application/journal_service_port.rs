//! Adapter forwarding for the owned journal application service.
use super::journal_service::JournalService;
use crate::journal::domain::journal_api::JournalApi;
use crate::shared_kernel::{
    append_result::AppendResult,
    event::{EventEnvelope, SignalKind},
    event_query::EventQuery,
    stored_event::StoredEvent,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Arc;
impl JournalApi for JournalService<'static> {
    type Error = anyhow::Error;
    fn ensure_queryable(&self) -> Result<()> {
        self.api().ensure_queryable()
    }
    fn set_retention_fenced(&self, fenced: bool) {
        self.api().set_retention_fenced(fenced)
    }
    fn mark_recovery_required(&self) {
        self.api().mark_recovery_required()
    }
    fn recovery_required(&self) -> bool {
        self.api().recovery_required()
    }
    fn retention_generation(&self) -> u64 {
        self.api().retention_generation()
    }
    fn govern_event(&self, event: EventEnvelope) -> Result<EventEnvelope> {
        self.api().govern_event(event)
    }
    fn last_cursor(&self) -> u64 {
        self.api().last_cursor()
    }
    fn projection_generation(&self) -> u64 {
        self.api().projection_generation()
    }
    fn data_dir(&self) -> &Path {
        self.api().data_dir()
    }
    fn snapshot_bounds(&self) -> (u64, u64) {
        self.api().snapshot_bounds()
    }
    fn resident_event_count(&self) -> usize {
        self.api().resident_event_count()
    }
    fn total_event_count(&self) -> u64 {
        self.api().total_event_count()
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
        self.api().seal_archive_prefix()
    }
    fn compact_archived_wal(
        &self,
        watermarks: crate::shared_kernel::archive_watermarks::ArchiveWatermarks,
    ) -> Result<()> {
        self.api().compact_archived_wal(watermarks)
    }
    fn scan_blob_references_page(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<(Vec<String>, u64, bool)> {
        self.api().scan_blob_references_page(after, limit)
    }
    fn finalize_blob_candidates_with_index(
        &self,
        hashes: &[String],
        after: u64,
        limit: usize,
        mark_live: &mut dyn FnMut(&str) -> Result<()>,
        is_live: &mut dyn FnMut(&str) -> Result<bool>,
    ) -> Result<(u64, usize, bool)> {
        self.api()
            .finalize_blob_candidates_with_index(hashes, after, limit, mark_live, is_live)
    }
    fn snapshot_bytes(&self) -> Result<Vec<u8>> {
        self.api().snapshot_bytes()
    }
    fn restore_snapshot_bytes(&self, bytes: &[u8]) -> Result<()> {
        self.api().restore_snapshot_bytes(bytes)
    }
    fn append(&self, event: EventEnvelope) -> Result<AppendResult> {
        self.api().append(event)
    }
    fn append_durable_batch_at(
        &self,
        events: Vec<EventEnvelope>,
        acknowledged_at: DateTime<Utc>,
    ) -> Result<Vec<AppendResult>> {
        self.api().append_durable_batch_at(events, acknowledged_at)
    }
    fn result_for_at(
        &self,
        project: &str,
        event_id: &str,
        decision_time: DateTime<Utc>,
    ) -> Result<Option<AppendResult>> {
        self.api().result_for_at(project, event_id, decision_time)
    }
    fn query(&self, query: EventQuery) -> Result<Vec<StoredEvent>> {
        self.api().query(query)
    }
    fn query_unchecked(&self, query: EventQuery) -> Result<Vec<StoredEvent>> {
        self.api().query_unchecked(query)
    }
    fn query_projection_events(&self, after: u64, limit: usize) -> Result<Vec<StoredEvent>> {
        self.api().query_projection_events(after, limit)
    }
    fn replay(&self, after: u64, limit: usize) -> Result<Vec<StoredEvent>> {
        self.api().replay(after, limit)
    }
    fn adopt_archive_checkpoint(
        &self,
        restored: &dyn JournalApi<Error = Self::Error>,
        receipt: &crate::shared_kernel::archive_receipts::ArchiveReceipt,
        expected_raw_cursor: u64,
    ) -> Result<()> {
        self.api()
            .adopt_archive_checkpoint(restored, receipt, expected_raw_cursor)
    }
    fn adopt_archive_retention_delta(
        &self,
        receipt: &crate::shared_kernel::archive_receipts::ArchiveReceipt,
        expected_raw_cursor: u64,
    ) -> Result<()> {
        self.api()
            .adopt_archive_retention_delta(receipt, expected_raw_cursor)
    }
    fn adopt_archive_coverage(
        &self,
        receipt: &crate::shared_kernel::archive_receipts::ArchiveReceipt,
        expected_raw_cursor: u64,
    ) -> Result<()> {
        self.api()
            .adopt_archive_coverage(receipt, expected_raw_cursor)
    }
    fn restore_stored_page(&self, events: Vec<StoredEvent>) -> Result<()> {
        self.api().restore_stored_page(events)
    }
    fn restore_archive_dedupe_page(&self, events: &[StoredEvent]) -> Result<()> {
        self.api().restore_archive_dedupe_page(events)
    }
    fn restore_archive_receipts(
        &self,
        receipts: &[crate::shared_kernel::dedupe_receipt::DedupeReceipt],
        indexed_through_cursor: u64,
    ) -> Result<()> {
        self.api()
            .restore_archive_receipts(receipts, indexed_through_cursor)
    }
    fn set_restored_archive_head(
        &self,
        manifest: &crate::shared_kernel::archive_manifest::ArchiveManifest,
    ) -> Result<()> {
        self.api().set_restored_archive_head(manifest)
    }
    fn evict_resident_before(&self, cutoff: DateTime<Utc>) -> Result<usize> {
        self.api().evict_resident_before(cutoff)
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
        self.api().apply_expiration_head(
            cutoff,
            archived_prefix_events,
            retained_prefix_events,
            archived_prefix_digest,
            retained_prefix_digest,
            repair_dedupe,
        )
    }
    fn checkpoint_identity(&self, raw_cursor: u64) -> Result<(u64, [u8; 32], u64)> {
        self.api().checkpoint_identity(raw_cursor)
    }
    fn maintain_dedupe_at(&self, _now: DateTime<Utc>, force: bool) -> Result<usize> {
        self.api().maintain_dedupe_at(_now, force)
    }
    fn event_store(
        &self,
    ) -> Arc<dyn crate::journal::domain::event_store::RawEventStore<Error = Self::Error>> {
        self.api().event_store()
    }
    fn storage(
        &self,
    ) -> &dyn crate::journal::domain::event_store::RawEventStore<Error = Self::Error> {
        self.api().storage()
    }
    fn dedupe(
        &self,
    ) -> &dyn crate::journal::domain::dedupe_store::DedupeStore<Error = Self::Error> {
        self.api().dedupe()
    }
    fn archive_read(
        &self,
    ) -> &dyn crate::journal::domain::archive_access::ArchiveRead<Error = Self::Error> {
        self.api().archive_read()
    }
    fn archive_maintenance(
        &self,
    ) -> &dyn crate::journal::domain::archive_access::ArchiveMaintenance<Error = Self::Error> {
        self.api().archive_maintenance()
    }
    fn open_staging(&self, root: &Path) -> Result<Arc<dyn JournalApi<Error = Self::Error>>> {
        self.api().open_staging(root)
    }
    fn metrics_snapshot(&self) -> [u64; 4] {
        self.api().metrics_snapshot()
    }
    fn projection_read_session(
        self: Arc<Self>,
        after: u64,
    ) -> Result<
        Box<dyn crate::journal::domain::projection_reader::ProjectionReader<Error = Self::Error>>,
    > {
        self.port().projection_read_session(after)
    }
}
