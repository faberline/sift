//! Journal use cases over an owned or borrowed domain port.

use crate::journal::application::raw_event_service::RawEventService;
use crate::journal::domain::journal_api::JournalApi;
use crate::shared_kernel::append_result::AppendResult;
use crate::shared_kernel::event::{EventEnvelope, SignalKind};
use crate::shared_kernel::event_query::EventQuery;
use crate::shared_kernel::stored_event::StoredEvent;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Arc;

type Api<'a> = dyn JournalApi<Error = anyhow::Error> + 'a;
enum JournalReference<'a> {
    Owned(Arc<Api<'static>>),
    Borrowed(&'a Api<'a>),
}
pub struct JournalService<'a> {
    inner: JournalReference<'a>,
    storage: RawEventService,
}
pub type DurableJournal = JournalService<'static>;

impl JournalService<'_> {
    pub(crate) fn metrics_snapshot(&self) -> [u64; 4] {
        self.api().metrics_snapshot()
    }
    pub(super) fn api(&self) -> &Api<'_> {
        match &self.inner {
            JournalReference::Owned(api) => api.as_ref(),
            JournalReference::Borrowed(api) => *api,
        }
    }
    pub fn storage(&self) -> &RawEventService {
        &self.storage
    }
}
impl<'a> JournalService<'a> {
    pub(crate) fn borrowed(api: &'a Api<'a>) -> Self {
        Self {
            storage: RawEventService::new(api.event_store()),
            inner: JournalReference::Borrowed(api),
        }
    }
}
impl JournalService<'static> {
    pub(crate) fn port(&self) -> Arc<Api<'static>> {
        match &self.inner {
            JournalReference::Owned(api) => api.clone(),
            JournalReference::Borrowed(_) => unreachable!("owned journal service"),
        }
    }

    pub(crate) fn owned(api: Arc<Api<'static>>) -> Self {
        Self {
            storage: RawEventService::new(api.event_store()),
            inner: JournalReference::Owned(api),
        }
    }
}

impl JournalService<'_> {
    pub(crate) fn ensure_queryable(&self) -> Result<()> {
        self.api().ensure_queryable()
    }
    pub(crate) fn mark_recovery_required(&self) {
        self.api().mark_recovery_required()
    }
    pub(crate) fn recovery_required(&self) -> bool {
        self.api().recovery_required()
    }
    pub(crate) fn retention_generation(&self) -> u64 {
        self.api().retention_generation()
    }
    pub fn govern_event(&self, event: EventEnvelope) -> Result<EventEnvelope> {
        self.api().govern_event(event)
    }
    pub(crate) fn last_cursor(&self) -> u64 {
        self.api().last_cursor()
    }
    pub(crate) fn projection_generation(&self) -> u64 {
        self.api().projection_generation()
    }
    pub(crate) fn data_dir(&self) -> &Path {
        self.api().data_dir()
    }
    pub fn resident_event_count(&self) -> usize {
        self.api().resident_event_count()
    }
    pub fn total_event_count(&self) -> u64 {
        self.api().total_event_count()
    }
    pub(crate) fn seal_archive_prefix(
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
    pub(crate) fn compact_archived_wal(
        &self,
        watermarks: crate::shared_kernel::archive_watermarks::ArchiveWatermarks,
    ) -> Result<()> {
        self.api().compact_archived_wal(watermarks)
    }
    pub(crate) fn scan_blob_references_page(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<(Vec<String>, u64, bool)> {
        self.api().scan_blob_references_page(after, limit)
    }
    pub(crate) fn finalize_blob_candidates_with_index<Mark, IsLive>(
        &self,
        hashes: &[String],
        after: u64,
        limit: usize,
        mut mark_live: Mark,
        mut is_live: IsLive,
    ) -> Result<(u64, usize, bool)>
    where
        Mark: FnMut(&str) -> Result<()>,
        IsLive: FnMut(&str) -> Result<bool>,
    {
        self.api().finalize_blob_candidates_with_index(
            hashes,
            after,
            limit,
            &mut mark_live,
            &mut is_live,
        )
    }
    pub fn snapshot_bytes(&self) -> Result<Vec<u8>> {
        self.api().snapshot_bytes()
    }
    pub fn restore_snapshot_bytes(&self, bytes: &[u8]) -> Result<()> {
        self.api().restore_snapshot_bytes(bytes)
    }
    pub fn append(&self, event: EventEnvelope) -> Result<AppendResult> {
        self.api().append(event)
    }
    pub(crate) fn result_for_at(
        &self,
        project: &str,
        event_id: &str,
        decision_time: DateTime<Utc>,
    ) -> Result<Option<AppendResult>> {
        self.api().result_for_at(project, event_id, decision_time)
    }
    pub fn query(&self, query: EventQuery) -> Result<Vec<StoredEvent>> {
        self.api().query(query)
    }
    pub(crate) fn query_projection_events(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<Vec<StoredEvent>> {
        self.api().query_projection_events(after, limit)
    }
    pub fn replay(&self, after: u64, limit: usize) -> Result<Vec<StoredEvent>> {
        self.api().replay(after, limit)
    }
    pub(crate) fn restore_stored_page(&self, events: Vec<StoredEvent>) -> Result<()> {
        self.api().restore_stored_page(events)
    }
    pub(crate) fn restore_archive_dedupe_page(&self, events: &[StoredEvent]) -> Result<()> {
        self.api().restore_archive_dedupe_page(events)
    }
    pub(crate) fn restore_archive_receipts(
        &self,
        receipts: &[crate::shared_kernel::dedupe_receipt::DedupeReceipt],
        indexed_through_cursor: u64,
    ) -> Result<()> {
        self.api()
            .restore_archive_receipts(receipts, indexed_through_cursor)
    }
    pub(crate) fn set_restored_archive_head(
        &self,
        manifest: &crate::shared_kernel::archive_manifest::ArchiveManifest,
    ) -> Result<()> {
        self.api().set_restored_archive_head(manifest)
    }
    pub(crate) fn evict_resident_before(&self, cutoff: DateTime<Utc>) -> Result<usize> {
        self.api().evict_resident_before(cutoff)
    }
    pub(crate) fn apply_expiration_head(
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
    pub(crate) fn maintain_dedupe_at(&self, _now: DateTime<Utc>, force: bool) -> Result<usize> {
        self.api().maintain_dedupe_at(_now, force)
    }
}
