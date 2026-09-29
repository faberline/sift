//! Append CRC frames per epoch/shard, recover torn tails, seal manifests, and
//! move immutable segments without rewriting bytes.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{bail, Context, Result};

use crate::journal::domain::segment_manifest::{AppendLocation, SegmentManifest};
use crate::journal::domain::shard_route::{bucket_for, Route};
use crate::journal::infrastructure::storage::segment_files::{
    event_time_unix_nano, files_with_extension, read_events, route_from_path, verify_segment,
};
use crate::shared_kernel::stored_event::StoredEvent;

pub(super) const FRAMED_LOG_HEADER_BYTES: u64 = 16;

pub(super) struct ActiveSegment {
    pub(super) route: Route,
    pub(super) segment_id: String,
    pub(super) path: PathBuf,
    pub(super) writer: storage_durable::FramedLogWriter,
    pub(super) first_cursor: u64,
    pub(super) last_cursor: u64,
    pub(super) event_count: u64,
    pub(super) min_event_time_unix_nano: i64,
    pub(super) max_event_time_unix_nano: i64,
    pub(super) encoded_bytes: u64,
    pub(super) bucket_min: u16,
    pub(super) bucket_max: u16,
}

#[derive(Default)]
pub(super) struct SegmentStateData {
    pub(super) active: HashMap<(u64, u16), ActiveSegment>,
    pub(super) sealed: HashMap<String, SegmentManifest>,
    pub(super) cursors: HashMap<u64, (String, AppendLocation)>,
}

pub struct SegmentStore {
    pub(super) root: PathBuf,
    pub(super) manifests_root: PathBuf,
    pub(super) max_segment_events: usize,
    pub(super) max_segment_bytes: usize,
    pub(super) inner: Mutex<SegmentStateData>,
}

impl SegmentStore {
    pub(crate) fn open_at(
        root: PathBuf,
        max_segment_events: usize,
        max_segment_bytes: usize,
    ) -> Result<Self> {
        if max_segment_events == 0 {
            bail!("max_segment_events must be greater than zero");
        }
        if max_segment_bytes == 0 {
            bail!("max_segment_bytes must be greater than zero");
        }
        let manifests_root = root.join("manifests");
        fs::create_dir_all(&manifests_root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        fs::set_permissions(&manifests_root, fs::Permissions::from_mode(0o700))?;
        let store = Self {
            root,
            manifests_root,
            max_segment_events,
            max_segment_bytes,
            inner: Mutex::new(SegmentStateData::default()),
        };
        store.load()?;
        Ok(store)
    }

    fn load(&self) -> Result<()> {
        let mut state = self.inner.lock().expect("segment state lock poisoned");
        for path in files_with_extension(&self.manifests_root, "json")? {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            let manifest: SegmentManifest = serde_json::from_slice(&fs::read(&path)?)
                .with_context(|| format!("decode segment manifest {}", path.display()))?;
            fs::set_permissions(&manifest.local_path, fs::Permissions::from_mode(0o600))?;
            verify_segment(&manifest)?;
            state.sealed.insert(manifest.segment_id.clone(), manifest);
        }
        let sealed_paths = state
            .sealed
            .values()
            .map(|manifest| manifest.local_path.clone())
            .collect::<HashSet<_>>();
        for path in files_with_extension(&self.root, "open")? {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            if sealed_paths.contains(&path) {
                continue;
            }
            let (epoch, shard) = route_from_path(&path)?;
            let writer = storage_durable::FramedLogWriter::open(
                &path,
                storage_durable::FsyncPolicy::Interval,
            )?;
            if let Some(shard_root) = path.parent() {
                fs::set_permissions(shard_root, fs::Permissions::from_mode(0o700))?;
                if let Some(epoch_root) = shard_root.parent() {
                    fs::set_permissions(epoch_root, fs::Permissions::from_mode(0o700))?;
                }
            }
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            let events = read_events(&path)?;
            if events.is_empty() {
                continue;
            }
            let encoded_bytes = fs::metadata(&path)?.len();
            let segment_id = path
                .file_stem()
                .and_then(|value| value.to_str())
                .context("active segment file has invalid name")?
                .to_string();
            let mut bucket_min = u16::MAX;
            let mut bucket_max = 0;
            let mut min_event_time_unix_nano = i64::MAX;
            let mut max_event_time_unix_nano = i64::MIN;
            for event in &events {
                let bucket = bucket_for(&event.event.event_id);
                let event_time = event_time_unix_nano(event)?;
                bucket_min = bucket_min.min(bucket);
                bucket_max = bucket_max.max(bucket);
                min_event_time_unix_nano = min_event_time_unix_nano.min(event_time);
                max_event_time_unix_nano = max_event_time_unix_nano.max(event_time);
                self.index_event(
                    &mut state,
                    event,
                    AppendLocation {
                        route: Route {
                            epoch,
                            shard,
                            bucket,
                        },
                        segment_id: segment_id.clone(),
                        path: path.clone(),
                    },
                )?;
            }
            state.active.insert(
                (epoch, shard),
                ActiveSegment {
                    route: Route {
                        epoch,
                        shard,
                        bucket: bucket_min,
                    },
                    segment_id,
                    path,
                    writer,
                    first_cursor: events.first().unwrap().cursor,
                    last_cursor: events.last().unwrap().cursor,
                    event_count: events.len() as u64,
                    min_event_time_unix_nano,
                    max_event_time_unix_nano,
                    encoded_bytes,
                    bucket_min,
                    bucket_max,
                },
            );
        }
        let full_segments = state
            .active
            .iter()
            .filter_map(|(key, active)| self.segment_is_ready(active).then_some(*key))
            .collect::<Vec<_>>();
        for key in full_segments {
            self.seal_locked(&mut state, key)?;
        }
        Ok(())
    }

    pub(super) fn index_event(
        &self,
        state: &mut SegmentStateData,
        event: &StoredEvent,
        location: AppendLocation,
    ) -> Result<()> {
        if let Some((existing_id, _)) = state.cursors.get(&event.cursor) {
            if existing_id != &event.event.event_id {
                bail!(
                    "raw segments contain conflicting cursor {} for {} and {}",
                    event.cursor,
                    existing_id,
                    event.event.event_id
                );
            }
            return Ok(());
        }
        state
            .cursors
            .insert(event.cursor, (event.event.event_id.clone(), location));
        Ok(())
    }
}
