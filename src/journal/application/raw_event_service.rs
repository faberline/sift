//! Application access to the canonical event store.

use crate::journal::domain::event_store::RawEventStore;
use crate::shared_kernel::retained_prefix_reconcile_stats::RetainedPrefixReconcileStats;
use crate::shared_kernel::segment_manifest::{AppendLocation, SegmentManifest};
use crate::shared_kernel::shard_route::{EpochMap, Route};
use crate::shared_kernel::stored_event::StoredEvent;
use crate::{ContentBlobRef, OperationalEventV2, SignalKind};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct RawEventService {
    pub(in crate::journal::application) backend: Arc<dyn RawEventStore<Error = anyhow::Error>>,
}
impl RawEventService {
    pub(crate) fn new(backend: Arc<dyn RawEventStore<Error = anyhow::Error>>) -> Self {
        Self { backend }
    }
    pub fn externalize_event(&self, event: &mut OperationalEventV2) -> Result<()> {
        self.backend.externalize_event(event)
    }
    pub fn append(&self, stored: &StoredEvent) -> Result<AppendLocation> {
        self.backend.append(stored)
    }
    pub fn append_batch(&self, events: &[StoredEvent]) -> Result<Vec<AppendLocation>> {
        self.backend.append_batch(events)
    }
    pub fn seal_ready(&self) -> Result<Vec<SegmentManifest>> {
        self.backend.seal_ready()
    }
    pub fn route(&self, event_id: &str, cursor: u64) -> Route {
        self.backend.route(event_id, cursor)
    }
    pub fn activate_epoch(&self, activated_at_cursor: u64, buckets: Vec<u16>) -> Result<EpochMap> {
        self.backend.activate_epoch(activated_at_cursor, buckets)
    }
    pub fn epoch_maps(&self) -> Vec<EpochMap> {
        self.backend.epoch_maps()
    }
    pub fn recovered_events(&self) -> Result<Vec<StoredEvent>> {
        self.backend.recovered_events()
    }
    pub fn query_events(
        &self,
        signal: Option<SignalKind>,
        after: u64,
        limit: usize,
    ) -> Result<Vec<StoredEvent>> {
        self.backend.query_events(signal, after, limit)
    }
    pub fn seal_all(&self) -> Result<Vec<SegmentManifest>> {
        self.backend.seal_all()
    }
    pub fn seal_all_with_signal(&self) -> Result<Vec<(SignalKind, SegmentManifest)>> {
        self.backend.seal_all_with_signal()
    }
    pub fn read_segment_events(
        &self,
        signal: SignalKind,
        manifest: &SegmentManifest,
    ) -> Result<Vec<StoredEvent>> {
        self.backend.read_segment_events(signal, manifest)
    }
    pub fn manifests(&self) -> Result<Vec<SegmentManifest>> {
        self.backend.manifests()
    }
    pub fn active_segment_paths(&self) -> Vec<PathBuf> {
        self.backend.active_segment_paths()
    }
    pub fn move_segment(
        &self,
        segment_id: &str,
        destination: impl AsRef<Path>,
    ) -> Result<SegmentManifest> {
        self.backend.move_segment(segment_id, destination.as_ref())
    }
    pub fn read_blob(&self, hash: &str) -> Result<Vec<u8>> {
        self.backend.read_blob(hash)
    }
    pub fn validate_blob_refs(&self, references: &[ContentBlobRef]) -> Result<()> {
        self.backend.validate_blob_refs(references)
    }
    pub fn blob_paths(&self) -> Result<Vec<PathBuf>> {
        self.backend.blob_paths()
    }
    pub fn root(&self) -> &Path {
        self.backend.root()
    }
    pub(crate) fn evict_expired_before(
        &self,
        cutoff: DateTime<Utc>,
        snapshot_index: u64,
    ) -> Result<u64> {
        self.backend.evict_expired_before(cutoff, snapshot_index)
    }
    pub(crate) fn evict_segment(
        &self,
        signal: SignalKind,
        segment_id: &str,
    ) -> Result<Option<SegmentManifest>> {
        self.backend.evict_segment(signal, segment_id)
    }
    pub fn reconcile_retained_prefix(
        &self,
        retained: &RawEventService,
        snapshot_index: u64,
    ) -> Result<RetainedPrefixReconcileStats> {
        self.backend
            .reconcile_retained_prefix(retained.backend.as_ref(), snapshot_index)
    }
}
