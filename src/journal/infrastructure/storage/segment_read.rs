//! Reading events back out of segments.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::journal::domain::segment_manifest::SegmentManifest;
use crate::journal::infrastructure::storage::segment_files::{
    read_events, read_events_after, verify_segment,
};
use crate::journal::infrastructure::storage::segment_store::SegmentStore;
use crate::shared_kernel::stored_event::StoredEvent;

pub(crate) struct SegmentEventReader {
    paths: VecDeque<PathBuf>,
    current: Option<storage_durable::FramedLogCursor>,
    after: u64,
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
        Ok(SegmentEventReader {
            paths: paths.into_iter().map(|(_, _, path)| path).collect(),
            current: None,
            after,
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

impl SegmentEventReader {
    pub(crate) fn next_event(&mut self) -> Result<Option<StoredEvent>> {
        loop {
            if self.current.is_none() {
                let Some(path) = self.paths.pop_front() else {
                    return Ok(None);
                };
                self.current = Some(storage_durable::FramedLogCursor::open(path)?);
            }
            let Some(frame) = self
                .current
                .as_mut()
                .expect("segment cursor exists")
                .next_frame()?
            else {
                self.current = None;
                continue;
            };
            let event: StoredEvent = serde_json::from_slice(&frame.payload)?;
            if event.cursor != frame.seq {
                bail!(
                    "segment frame {} contains cursor {}",
                    frame.seq,
                    event.cursor
                );
            }
            if event.cursor <= self.after {
                continue;
            }
            self.after = event.cursor;
            return Ok(Some(event));
        }
    }
}
