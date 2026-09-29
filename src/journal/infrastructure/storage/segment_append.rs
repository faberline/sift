//! Appending to the active segments, and sealing them into manifests.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use anyhow::{bail, Context, Result};

use crate::journal::domain::segment_manifest::{AppendLocation, SegmentManifest, SegmentState};
use crate::journal::domain::shard_route::Route;
use crate::journal::infrastructure::storage::segment_files::{event_time_unix_nano, sha256_file};
use crate::journal::infrastructure::storage::segment_store::{
    ActiveSegment, SegmentStateData, SegmentStore, FRAMED_LOG_HEADER_BYTES,
};
use crate::shared_kernel::stored_event::StoredEvent;

impl SegmentStore {
    pub fn append(&self, route: Route, stored: &StoredEvent) -> Result<AppendLocation> {
        let mut state = self.inner.lock().expect("segment state lock poisoned");
        if let Some((event_id, location)) = state.cursors.get(&stored.cursor) {
            if event_id != &stored.event.event_id {
                bail!("cursor {} already belongs to {event_id}", stored.cursor);
            }
            return Ok(location.clone());
        }
        let key = (route.epoch, route.shard);
        let event_time = event_time_unix_nano(stored)?;
        if let std::collections::hash_map::Entry::Vacant(entry) = state.active.entry(key) {
            let segment_id = format!(
                "segment-e{:020}-s{:04}-c{:020}",
                route.epoch, route.shard, stored.cursor
            );
            let path = self
                .root
                .join(format!("epoch-{:020}", route.epoch))
                .join(format!("shard-{:04}", route.shard))
                .join(format!("{segment_id}.open"));
            let writer = storage_durable::FramedLogWriter::open(
                &path,
                storage_durable::FsyncPolicy::Interval,
            )?;
            entry.insert(ActiveSegment {
                route,
                segment_id,
                path,
                writer,
                first_cursor: stored.cursor,
                last_cursor: stored.cursor,
                event_count: 0,
                min_event_time_unix_nano: event_time,
                max_event_time_unix_nano: event_time,
                encoded_bytes: 0,
                bucket_min: route.bucket,
                bucket_max: route.bucket,
            });
        }
        let encoded = serde_json::to_vec(stored)?;
        let (location, should_seal) = {
            let active = state.active.get_mut(&key).unwrap();
            active.writer.append(stored.cursor, &encoded)?;
            active.last_cursor = stored.cursor;
            active.event_count += 1;
            active.min_event_time_unix_nano = active.min_event_time_unix_nano.min(event_time);
            active.max_event_time_unix_nano = active.max_event_time_unix_nano.max(event_time);
            active.encoded_bytes = active
                .encoded_bytes
                .saturating_add(FRAMED_LOG_HEADER_BYTES)
                .saturating_add(encoded.len() as u64);
            active.bucket_min = active.bucket_min.min(route.bucket);
            active.bucket_max = active.bucket_max.max(route.bucket);
            (
                AppendLocation {
                    route,
                    segment_id: active.segment_id.clone(),
                    path: active.path.clone(),
                },
                self.segment_is_ready(active),
            )
        };
        self.index_event(&mut state, stored, location.clone())?;
        // Sealing performs fsync, hashing, rename, and manifest replacement.
        // Keep it out of the acknowledgement path. The background storage
        // worker calls `seal_ready`; an explicit archive calls `seal_all`.
        let _ = should_seal;
        Ok(location)
    }

    pub fn seal_ready(&self) -> Result<Vec<SegmentManifest>> {
        let mut state = self.inner.lock().expect("segment state lock poisoned");
        let keys = state
            .active
            .iter()
            .filter_map(|(key, active)| self.segment_is_ready(active).then_some(*key))
            .collect::<Vec<_>>();
        let mut manifests = Vec::with_capacity(keys.len());
        for key in keys {
            manifests.push(self.seal_locked(&mut state, key)?);
        }
        Ok(manifests)
    }

    pub fn flush_active(&self) -> Result<()> {
        let mut state = self.inner.lock().expect("segment state lock poisoned");
        for active in state.active.values_mut() {
            active.writer.flush()?;
        }
        Ok(())
    }

    pub(super) fn segment_is_ready(&self, active: &ActiveSegment) -> bool {
        active.event_count as usize >= self.max_segment_events
            || active.encoded_bytes >= self.max_segment_bytes as u64
    }

    pub(super) fn seal_locked(
        &self,
        state: &mut SegmentStateData,
        key: (u64, u16),
    ) -> Result<SegmentManifest> {
        let mut active = state
            .active
            .remove(&key)
            .context("active segment disappeared before seal")?;
        active.writer.sync()?;
        drop(active.writer);
        let sealed_path = active.path.with_extension("framed");
        fs::rename(&active.path, &sealed_path)?;
        storage_durable::sync_parent_dir(&sealed_path)?;
        let bytes = fs::metadata(&sealed_path)?.len();
        let manifest = SegmentManifest {
            segment_id: active.segment_id,
            epoch: active.route.epoch,
            shard: active.route.shard,
            bucket_min: active.bucket_min,
            bucket_max: active.bucket_max,
            first_cursor: active.first_cursor,
            last_cursor: active.last_cursor,
            event_count: active.event_count,
            min_event_time_unix_nano: active.min_event_time_unix_nano,
            max_event_time_unix_nano: active.max_event_time_unix_nano,
            bytes,
            sha256: sha256_file(&sealed_path)?,
            state: SegmentState::Sealed,
            local_path: sealed_path.clone(),
            object_uri: None,
        };
        self.write_manifest(&manifest)?;
        state
            .cursors
            .retain(|_, (_, location)| location.segment_id != manifest.segment_id);
        state
            .sealed
            .insert(manifest.segment_id.clone(), manifest.clone());
        Ok(manifest)
    }

    pub(super) fn write_manifest(&self, manifest: &SegmentManifest) -> Result<()> {
        let path = self
            .manifests_root
            .join(format!("{}.json", manifest.segment_id));
        storage_durable::atomic_write(
            &path,
            &serde_json::to_vec_pretty(manifest)?,
            storage_durable::FsyncPolicy::Always,
        )?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    pub fn seal_all(&self) -> Result<Vec<SegmentManifest>> {
        let mut state = self.inner.lock().expect("segment state lock poisoned");
        let keys = state.active.keys().copied().collect::<Vec<_>>();
        for key in keys {
            self.seal_locked(&mut state, key)?;
        }
        let mut manifests = state.sealed.values().cloned().collect::<Vec<_>>();
        manifests.sort_by_key(|manifest| manifest.first_cursor);
        Ok(manifests)
    }

    pub fn manifests(&self) -> Result<Vec<SegmentManifest>> {
        let state = self.inner.lock().expect("segment state lock poisoned");
        let mut manifests = state.sealed.values().cloned().collect::<Vec<_>>();
        manifests.sort_by_key(|manifest| manifest.first_cursor);
        Ok(manifests)
    }
}
