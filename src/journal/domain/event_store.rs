//! Canonical event storage and bounded stream reads.

use crate::shared_kernel::retained_prefix_reconcile_stats::RetainedPrefixReconcileStats;
use crate::shared_kernel::segment_manifest::{AppendLocation, SegmentManifest};
use crate::shared_kernel::shard_route::{EpochMap, Route};
use crate::shared_kernel::stored_event::StoredEvent;
use crate::{ContentBlobRef, OperationalEventV2, SignalKind};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

pub trait EventReader: Send {
    type Error;
    fn next_event(&mut self) -> Result<Option<StoredEvent>, Self::Error>;
}

pub trait RawEventStore: Send + Sync {
    type Error;
    fn externalize_event(&self, event: &mut OperationalEventV2) -> Result<(), Self::Error>;
    fn append(&self, stored: &StoredEvent) -> Result<AppendLocation, Self::Error>;
    fn append_batch(&self, events: &[StoredEvent]) -> Result<Vec<AppendLocation>, Self::Error>;
    fn seal_ready(&self) -> Result<Vec<SegmentManifest>, Self::Error>;
    fn route(&self, event_id: &str, cursor: u64) -> Route;
    fn activate_epoch(
        &self,
        activated_at_cursor: u64,
        buckets: Vec<u16>,
    ) -> Result<EpochMap, Self::Error>;
    fn epoch_maps(&self) -> Vec<EpochMap>;
    fn recovered_events(&self) -> Result<Vec<StoredEvent>, Self::Error>;
    fn query_events(
        &self,
        signal: Option<SignalKind>,
        after: u64,
        limit: usize,
    ) -> Result<Vec<StoredEvent>, Self::Error>;
    fn reader(&self, after: u64) -> Result<Box<dyn EventReader<Error = Self::Error>>, Self::Error>;
    fn seal_all(&self) -> Result<Vec<SegmentManifest>, Self::Error>;
    fn seal_all_with_signal(&self) -> Result<Vec<(SignalKind, SegmentManifest)>, Self::Error>;
    fn read_segment_events(
        &self,
        signal: SignalKind,
        manifest: &SegmentManifest,
    ) -> Result<Vec<StoredEvent>, Self::Error>;
    fn manifests(&self) -> Result<Vec<SegmentManifest>, Self::Error>;
    fn active_segment_paths(&self) -> Vec<PathBuf>;
    fn move_segment(
        &self,
        segment_id: &str,
        destination: &Path,
    ) -> Result<SegmentManifest, Self::Error>;
    fn read_blob(&self, hash: &str) -> Result<Vec<u8>, Self::Error>;
    fn validate_blob_refs(&self, references: &[ContentBlobRef]) -> Result<(), Self::Error>;
    fn blob_paths(&self) -> Result<Vec<PathBuf>, Self::Error>;
    fn remove_blob(&self, hash: &str) -> Result<bool, Self::Error>;
    fn root(&self) -> &Path;
    fn evict_expired_before(
        &self,
        cutoff: DateTime<Utc>,
        snapshot_index: u64,
    ) -> Result<u64, Self::Error>;
    fn evict_segment(
        &self,
        signal: SignalKind,
        segment_id: &str,
    ) -> Result<Option<SegmentManifest>, Self::Error>;
    fn reconcile_retained_prefix(
        &self,
        retained: &dyn RawEventStore<Error = Self::Error>,
        snapshot_index: u64,
    ) -> Result<RetainedPrefixReconcileStats, Self::Error>;
    fn signal_reader(
        &self,
        signal: SignalKind,
        after: u64,
    ) -> Result<Box<dyn EventReader<Error = Self::Error>>, Self::Error>;
}

impl<T: RawEventStore + ?Sized> RawEventStore for std::sync::Arc<T> {
    type Error = T::Error;
    fn externalize_event(&self, event: &mut OperationalEventV2) -> Result<(), Self::Error> {
        self.as_ref().externalize_event(event)
    }
    fn append(&self, stored: &StoredEvent) -> Result<AppendLocation, Self::Error> {
        self.as_ref().append(stored)
    }
    fn append_batch(&self, events: &[StoredEvent]) -> Result<Vec<AppendLocation>, Self::Error> {
        self.as_ref().append_batch(events)
    }
    fn seal_ready(&self) -> Result<Vec<SegmentManifest>, Self::Error> {
        self.as_ref().seal_ready()
    }
    fn route(&self, event_id: &str, cursor: u64) -> Route {
        self.as_ref().route(event_id, cursor)
    }
    fn activate_epoch(
        &self,
        activated_at_cursor: u64,
        buckets: Vec<u16>,
    ) -> Result<EpochMap, Self::Error> {
        self.as_ref().activate_epoch(activated_at_cursor, buckets)
    }
    fn epoch_maps(&self) -> Vec<EpochMap> {
        self.as_ref().epoch_maps()
    }
    fn recovered_events(&self) -> Result<Vec<StoredEvent>, Self::Error> {
        self.as_ref().recovered_events()
    }
    fn query_events(
        &self,
        signal: Option<SignalKind>,
        after: u64,
        limit: usize,
    ) -> Result<Vec<StoredEvent>, Self::Error> {
        self.as_ref().query_events(signal, after, limit)
    }
    fn reader(&self, after: u64) -> Result<Box<dyn EventReader<Error = Self::Error>>, Self::Error> {
        self.as_ref().reader(after)
    }
    fn seal_all(&self) -> Result<Vec<SegmentManifest>, Self::Error> {
        self.as_ref().seal_all()
    }
    fn seal_all_with_signal(&self) -> Result<Vec<(SignalKind, SegmentManifest)>, Self::Error> {
        self.as_ref().seal_all_with_signal()
    }
    fn read_segment_events(
        &self,
        signal: SignalKind,
        manifest: &SegmentManifest,
    ) -> Result<Vec<StoredEvent>, Self::Error> {
        self.as_ref().read_segment_events(signal, manifest)
    }
    fn manifests(&self) -> Result<Vec<SegmentManifest>, Self::Error> {
        self.as_ref().manifests()
    }
    fn active_segment_paths(&self) -> Vec<PathBuf> {
        self.as_ref().active_segment_paths()
    }
    fn move_segment(
        &self,
        segment_id: &str,
        destination: &Path,
    ) -> Result<SegmentManifest, Self::Error> {
        self.as_ref().move_segment(segment_id, destination)
    }
    fn read_blob(&self, hash: &str) -> Result<Vec<u8>, Self::Error> {
        self.as_ref().read_blob(hash)
    }
    fn validate_blob_refs(&self, references: &[ContentBlobRef]) -> Result<(), Self::Error> {
        self.as_ref().validate_blob_refs(references)
    }
    fn blob_paths(&self) -> Result<Vec<PathBuf>, Self::Error> {
        self.as_ref().blob_paths()
    }
    fn remove_blob(&self, hash: &str) -> Result<bool, Self::Error> {
        self.as_ref().remove_blob(hash)
    }
    fn root(&self) -> &Path {
        self.as_ref().root()
    }
    fn evict_expired_before(
        &self,
        cutoff: DateTime<Utc>,
        snapshot_index: u64,
    ) -> Result<u64, Self::Error> {
        self.as_ref().evict_expired_before(cutoff, snapshot_index)
    }
    fn evict_segment(
        &self,
        signal: SignalKind,
        segment_id: &str,
    ) -> Result<Option<SegmentManifest>, Self::Error> {
        self.as_ref().evict_segment(signal, segment_id)
    }
    fn reconcile_retained_prefix(
        &self,
        retained: &dyn RawEventStore<Error = Self::Error>,
        snapshot_index: u64,
    ) -> Result<RetainedPrefixReconcileStats, Self::Error> {
        self.as_ref()
            .reconcile_retained_prefix(retained, snapshot_index)
    }
    fn signal_reader(
        &self,
        signal: SignalKind,
        after: u64,
    ) -> Result<Box<dyn EventReader<Error = Self::Error>>, Self::Error> {
        self.as_ref().signal_reader(signal, after)
    }
}
