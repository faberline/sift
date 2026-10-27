//! Storage implements the event port without changing its locks or IO.

use crate::journal::domain::event_store::{EventReader, RawEventStore};
use crate::journal::infrastructure::storage::raw_storage::{RawStorage, RawStorageReader};
use crate::journal::infrastructure::storage::segment_read::SegmentEventReader;
use crate::shared_kernel::retained_prefix_reconcile_stats::RetainedPrefixReconcileStats;
use crate::shared_kernel::segment_manifest::{AppendLocation, SegmentManifest};
use crate::shared_kernel::shard_route::{EpochMap, Route};
use crate::shared_kernel::stored_event::StoredEvent;
use crate::{ContentBlobRef, OperationalEventV2, SignalKind};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

impl RawEventStore for RawStorage {
    type Error = anyhow::Error;
    fn externalize_event(&self, event: &mut OperationalEventV2) -> Result<()> {
        RawStorage::externalize_event(self, event)
    }
    fn append(&self, stored: &StoredEvent) -> Result<AppendLocation> {
        RawStorage::append(self, stored)
    }
    fn append_batch(&self, events: &[StoredEvent]) -> Result<Vec<AppendLocation>> {
        RawStorage::append_batch(self, events)
    }
    fn seal_ready(&self) -> Result<Vec<SegmentManifest>> {
        RawStorage::seal_ready(self)
    }
    fn route(&self, event_id: &str, cursor: u64) -> Route {
        RawStorage::route(self, event_id, cursor)
    }
    fn activate_epoch(&self, activated_at_cursor: u64, buckets: Vec<u16>) -> Result<EpochMap> {
        RawStorage::activate_epoch(self, activated_at_cursor, buckets)
    }
    fn epoch_maps(&self) -> Vec<EpochMap> {
        RawStorage::epoch_maps(self)
    }
    fn recovered_events(&self) -> Result<Vec<StoredEvent>> {
        RawStorage::recovered_events(self)
    }
    fn query_events(
        &self,
        signal: Option<SignalKind>,
        after: u64,
        limit: usize,
    ) -> Result<Vec<StoredEvent>> {
        RawStorage::query_events(self, signal, after, limit)
    }
    fn reader(&self, after: u64) -> Result<Box<dyn EventReader<Error = Self::Error>>> {
        Ok(Box::new(RawStorage::reader(self, after)?))
    }
    fn seal_all(&self) -> Result<Vec<SegmentManifest>> {
        RawStorage::seal_all(self)
    }
    fn seal_all_with_signal(&self) -> Result<Vec<(SignalKind, SegmentManifest)>> {
        RawStorage::seal_all_with_signal(self)
    }
    fn read_segment_events(
        &self,
        signal: SignalKind,
        manifest: &SegmentManifest,
    ) -> Result<Vec<StoredEvent>> {
        RawStorage::read_segment_events(self, signal, manifest)
    }
    fn manifests(&self) -> Result<Vec<SegmentManifest>> {
        RawStorage::manifests(self)
    }
    fn active_segment_paths(&self) -> Vec<PathBuf> {
        RawStorage::active_segment_paths(self)
    }
    fn move_segment(&self, segment_id: &str, destination: &Path) -> Result<SegmentManifest> {
        RawStorage::move_segment(self, segment_id, destination)
    }
    fn read_blob(&self, hash: &str) -> Result<Vec<u8>> {
        RawStorage::read_blob(self, hash)
    }
    fn validate_blob_refs(&self, references: &[ContentBlobRef]) -> Result<()> {
        RawStorage::validate_blob_refs(self, references)
    }
    fn blob_paths(&self) -> Result<Vec<PathBuf>> {
        RawStorage::blob_paths(self)
    }
    fn remove_blob(&self, hash: &str) -> Result<bool> {
        RawStorage::remove_blob(self, hash)
    }
    fn root(&self) -> &Path {
        RawStorage::root(self)
    }
    fn evict_expired_before(&self, cutoff: DateTime<Utc>, snapshot_index: u64) -> Result<u64> {
        RawStorage::evict_expired_before(self, cutoff, snapshot_index)
    }
    fn evict_segment(
        &self,
        signal: SignalKind,
        segment_id: &str,
    ) -> Result<Option<SegmentManifest>> {
        RawStorage::evict_segment(self, signal, segment_id)
    }
    fn reconcile_retained_prefix(
        &self,
        retained: &dyn RawEventStore<Error = Self::Error>,
        snapshot_index: u64,
    ) -> Result<RetainedPrefixReconcileStats> {
        RawStorage::reconcile_retained_prefix(self, retained, snapshot_index)
    }
    fn signal_reader(
        &self,
        signal: SignalKind,
        after: u64,
    ) -> Result<Box<dyn EventReader<Error = Self::Error>>> {
        Ok(Box::new(self.segments.for_signal(signal)?.reader(after)?))
    }
}
impl EventReader for RawStorageReader {
    type Error = anyhow::Error;
    fn next_event(&mut self) -> Result<Option<StoredEvent>> {
        RawStorageReader::next_event(self)
    }
}
impl EventReader for SegmentEventReader {
    type Error = anyhow::Error;
    fn next_event(&mut self) -> Result<Option<StoredEvent>> {
        SegmentEventReader::next_event(self)
    }
}
