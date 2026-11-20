//! Reading events back out of segments.

use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};

use crate::journal::infrastructure::storage::segment_files::{
    read_events, read_events_after, verify_segment,
};
use crate::journal::infrastructure::storage::segment_store::{SegmentStateData, SegmentStore};
use crate::shared_kernel::segment_manifest::SegmentManifest;
use crate::shared_kernel::stored_event::StoredEvent;

/// Most segment paths the reader holds at once; it loads the next page when
/// its queue is empty.
pub(crate) const SEGMENT_READER_MAX_PATHS: usize = 1_024;

pub(crate) struct SegmentEventReader {
    paths: VecDeque<(PathBuf, u64)>,
    current: Option<storage_durable::FramedLogCursor>,
    current_last: u64,
    after: u64,
    state: Arc<Mutex<SegmentStateData>>,
    max_paths: usize,
    finished: bool,
}

/// The next page of segment paths that end after `after`, in order of their
/// first cursor. It keeps at most `max_paths` entries at any time.
fn next_page(
    state: &Mutex<SegmentStateData>,
    after: u64,
    max_paths: usize,
) -> VecDeque<(PathBuf, u64)> {
    let state = state.lock().expect("segment state lock poisoned");
    let mut heap = BinaryHeap::new();
    let entries = state
        .sealed
        .values()
        .map(|manifest| {
            (
                manifest.first_cursor,
                manifest.last_cursor,
                manifest.local_path.clone(),
            )
        })
        .chain(
            state
                .active
                .values()
                .map(|active| (active.first_cursor, active.last_cursor, active.path.clone())),
        );
    for (first, last, path) in entries {
        if last <= after {
            continue;
        }
        heap.push((first, last, path));
        if heap.len() > max_paths {
            heap.pop();
        }
    }
    drop(state);
    heap.into_sorted_vec()
        .into_iter()
        .map(|(_, last, path)| (path, last))
        .collect()
}

impl SegmentStore {
    pub fn query_events(&self, after: u64, limit: usize) -> Result<Vec<StoredEvent>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let state = self.inner.lock().expect("segment state lock poisoned");
        let mut paths = state
            .sealed
            .values()
            .map(|manifest| {
                (
                    manifest.first_cursor,
                    manifest.last_cursor,
                    manifest.local_path.clone(),
                )
            })
            .chain(
                state
                    .active
                    .values()
                    .map(|active| (active.first_cursor, active.last_cursor, active.path.clone())),
            )
            .filter(|(_, last, _)| *last > after)
            .collect::<Vec<_>>();
        drop(state);
        paths.sort_by_key(|(first, _, _)| *first);

        let mut events = Vec::with_capacity(limit.min(1_000));
        for (_, _, path) in paths {
            let remaining = limit.saturating_sub(events.len());
            events.extend(read_events_after(&path, after, remaining)?);
            if events.len() == limit {
                return Ok(events);
            }
        }
        Ok(events)
    }

    pub(crate) fn reader(&self, after: u64) -> Result<SegmentEventReader> {
        self.reader_with_max_paths(after, SEGMENT_READER_MAX_PATHS)
    }

    pub(crate) fn reader_with_max_paths(
        &self,
        after: u64,
        max_paths: usize,
    ) -> Result<SegmentEventReader> {
        if max_paths == 0 {
            bail!("segment reader page size must be greater than zero");
        }
        let state = Arc::clone(&self.inner);
        Ok(SegmentEventReader {
            paths: next_page(&state, after, max_paths),
            current: None,
            current_last: after,
            after,
            state,
            max_paths,
            finished: false,
        })
    }

    pub(crate) fn read_manifest_events(
        &self,
        manifest: &SegmentManifest,
    ) -> Result<Vec<StoredEvent>> {
        let state = self.inner.lock().expect("segment state lock poisoned");
        let owned = state
            .sealed
            .get(&manifest.segment_id)
            .filter(|owned| *owned == manifest)
            .is_some();
        drop(state);
        if !owned {
            bail!(
                "segment {} is not owned by this signal store",
                manifest.segment_id
            );
        }
        verify_segment(manifest)?;
        let events = read_events(&manifest.local_path)?;
        if events.len() as u64 != manifest.event_count
            || events.first().map(|event| event.cursor) != Some(manifest.first_cursor)
            || events.last().map(|event| event.cursor) != Some(manifest.last_cursor)
        {
            bail!(
                "segment {} content does not match its manifest",
                manifest.segment_id
            );
        }
        Ok(events)
    }

    pub fn recovered_events(&self) -> Result<Vec<StoredEvent>> {
        let state = self.inner.lock().expect("segment state lock poisoned");
        let mut paths = state
            .sealed
            .values()
            .map(|manifest| manifest.local_path.clone())
            .chain(state.active.values().map(|active| active.path.clone()))
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        drop(state);
        let mut by_cursor = HashMap::new();
        for path in paths {
            for event in read_events(&path)? {
                if let Some(existing) = by_cursor.insert(event.cursor, event.clone()) {
                    if existing.event.event_id != event.event.event_id {
                        bail!("conflicting raw segment cursor {}", event.cursor);
                    }
                }
            }
        }
        let mut events = by_cursor.into_values().collect::<Vec<_>>();
        events.sort_by_key(|event| event.cursor);
        Ok(events)
    }
}

#[cfg(test)]
impl SegmentEventReader {
    pub(crate) fn held_paths(&self) -> usize {
        self.paths.len()
    }
}

impl SegmentEventReader {
    pub(crate) fn next_event(&mut self) -> Result<Option<StoredEvent>> {
        loop {
            if self.finished {
                return Ok(None);
            }
            if self.current.is_none() {
                let next = match self.paths.pop_front() {
                    Some(next) => Some(next),
                    None => {
                        self.paths = next_page(&self.state, self.after, self.max_paths);
                        self.paths.pop_front()
                    }
                };
                let Some((path, last)) = next else {
                    self.finished = true;
                    return Ok(None);
                };
                if !path.exists() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("local segment is missing: {}", path.display()),
                    )
                    .into());
                }
                self.current_last = last;
                self.current = Some(storage_durable::FramedLogCursor::open(path)?);
            }
            let Some(frame) = self
                .current
                .as_mut()
                .expect("segment cursor exists")
                .next_frame()?
            else {
                if self.after < self.current_last {
                    bail!(
                        "local segment ended at cursor {} before committed cursor {}",
                        self.after,
                        self.current_last
                    );
                }
                self.current = None;
                continue;
            };
            if frame.seq() <= self.after {
                continue;
            }
            let event: StoredEvent = serde_json::from_slice(frame.payload())?;
            if event.cursor != frame.seq() {
                bail!(
                    "segment frame {} contains cursor {}",
                    frame.seq(),
                    event.cursor
                );
            }
            self.after = event.cursor;
            return Ok(Some(event));
        }
    }
}

#[cfg(test)]
mod paged_reader_tests {
    use super::*;
    use crate::shared_kernel::shard_route::Route;
    use crate::SignalKind;

    fn stored(cursor: u64) -> StoredEvent {
        let mut event = crate::EventEnvelope::for_project(
            "p",
            "prod",
            format!("e{cursor}"),
            SignalKind::Log,
            serde_json::json!({"message": "m"}),
        );
        event.resource =
            std::collections::BTreeMap::from([("service.name".to_string(), "svc".to_string())]);
        StoredEvent {
            cursor,
            acknowledged_at: event.observed_at.clone(),
            event,
        }
    }

    fn store_with_segments(root: &std::path::Path, count: u64) -> SegmentStore {
        let store = SegmentStore::open_at(root.to_path_buf(), 1, 1 << 30).unwrap();
        for cursor in 1..=count {
            store
                .append(
                    Route {
                        epoch: 1,
                        shard: 0,
                        bucket: 0,
                    },
                    &stored(cursor),
                )
                .unwrap();
            store.seal_ready().unwrap();
        }
        store
    }

    fn read_all(reader: &mut SegmentEventReader, max_held: usize) -> Vec<u64> {
        let mut cursors = Vec::new();
        while let Some(event) = reader.next_event().unwrap() {
            cursors.push(event.cursor);
            assert!(reader.held_paths() <= max_held);
        }
        cursors
    }

    #[test]
    fn reader_holds_at_most_one_page_of_paths_and_keeps_the_order() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_segments(dir.path(), 10);
        let mut reader = store.reader_with_max_paths(0, 3).unwrap();
        assert_eq!(reader.held_paths(), 3);
        assert_eq!(read_all(&mut reader, 3), (1..=10).collect::<Vec<u64>>());
    }

    #[test]
    fn reader_with_after_skips_earlier_segments() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_segments(dir.path(), 10);
        let mut reader = store.reader_with_max_paths(4, 3).unwrap();
        assert_eq!(read_all(&mut reader, 3), vec![5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn reader_with_page_larger_than_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_segments(dir.path(), 2);
        let mut reader = store.reader_with_max_paths(0, 1024).unwrap();
        assert_eq!(read_all(&mut reader, 1024), vec![1, 2]);
    }

    #[test]
    fn reader_with_zero_page_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_segments(dir.path(), 1);
        assert!(store.reader_with_max_paths(0, 0).is_err());
    }

    #[test]
    fn finished_reader_stays_finished() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_segments(dir.path(), 2);
        let mut reader = store.reader_with_max_paths(0, 1).unwrap();
        assert_eq!(read_all(&mut reader, 1), vec![1, 2]);
        store
            .append(
                Route {
                    epoch: 1,
                    shard: 0,
                    bucket: 0,
                },
                &stored(3),
            )
            .unwrap();
        store.seal_ready().unwrap();
        assert!(reader.next_event().unwrap().is_none());
    }
}
