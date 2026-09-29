//! Reading the canonical WAL and segments back in pages during recovery.

use anyhow::{bail, Context, Result};

use crate::journal::domain::journal_limits::{RECOVERY_PAGE_BYTES, RECOVERY_PAGE_EVENTS};
use crate::shared_kernel::stored_event::StoredEvent;

pub(in crate::journal) struct CanonicalRecoveryReader<'a> {
    pub(in crate::journal) storage:
        &'a crate::journal::infrastructure::storage::raw_storage::RawStorage,
    segments: crate::journal::infrastructure::storage::raw_storage::RawStorageReader,
    pub(in crate::journal) wal: crate::storage::SignalWalReader,
    archived: crate::shared_kernel::archive_watermarks::ArchiveWatermarks,
    repair_segments: bool,
    segment_next: Option<StoredEvent>,
    wal_next: Option<StoredEvent>,
    pending: Option<StoredEvent>,
}

impl<'a> CanonicalRecoveryReader<'a> {
    pub(in crate::journal) fn open(
        storage: &'a crate::journal::infrastructure::storage::raw_storage::RawStorage,
        wal: &'a crate::storage::SignalWal,
        archived: crate::shared_kernel::archive_watermarks::ArchiveWatermarks,
        after: u64,
        repair_segments: bool,
    ) -> Result<Self> {
        Ok(Self {
            storage,
            segments: storage.reader(after)?,
            wal: wal.reader(after)?,
            archived,
            repair_segments,
            segment_next: None,
            wal_next: None,
            pending: None,
        })
    }

    pub(in crate::journal) fn read_page(&mut self) -> Result<Vec<StoredEvent>> {
        self.read_page_with_limits(RECOVERY_PAGE_EVENTS, RECOVERY_PAGE_BYTES)
            .map(|(page, _)| page)
    }

    pub(super) fn read_page_with_limits(
        &mut self,
        max_events: usize,
        max_bytes: usize,
    ) -> Result<(Vec<StoredEvent>, bool)> {
        if max_events == 0 || max_bytes == 0 {
            bail!("canonical recovery page limits must be greater than zero");
        }
        let mut page = Vec::with_capacity(max_events.min(1_000));
        let mut bytes = 0_usize;
        while page.len() < max_events {
            let Some(event) = self.pending.take().or(self.next_event()?) else {
                return Ok((page, true));
            };
            let encoded = serde_json::to_vec(&event)?.len();
            if !page.is_empty() && bytes.saturating_add(encoded) > max_bytes {
                self.pending = Some(event);
                return Ok((page, false));
            }
            bytes = bytes.saturating_add(encoded);
            page.push(event);
        }
        Ok((page, false))
    }

    fn next_event(&mut self) -> Result<Option<StoredEvent>> {
        loop {
            if self.segment_next.is_none() {
                self.segment_next = self.segments.next_event()?;
            }
            if self.wal_next.is_none() {
                self.wal_next = self.wal.read_page(1, usize::MAX)?.pop();
            }
            let segment_cursor = self.segment_next.as_ref().map(|event| event.cursor);
            let wal_cursor = self.wal_next.as_ref().map(|event| event.cursor);
            match (segment_cursor, wal_cursor) {
                (None, None) => return Ok(None),
                (Some(segment_cursor), Some(wal_cursor)) if segment_cursor == wal_cursor => {
                    let segment = self.segment_next.take().expect("segment event exists");
                    let wal_event = self.wal_next.take().expect("WAL event exists");
                    if segment != wal_event {
                        bail!("WAL and segment disagree at cursor {segment_cursor}");
                    }
                    return Ok(Some(wal_event));
                }
                (Some(segment_cursor), Some(wal_cursor)) if segment_cursor < wal_cursor => {
                    let segment = self.segment_next.take().expect("segment event exists");
                    if !self.archived.covers(segment.event.signal, segment_cursor) {
                        bail!(
                            "segment cursor {segment_cursor} has no committed WAL or archive receipt"
                        );
                    }
                    return Ok(Some(segment));
                }
                (Some(_), Some(_)) | (None, Some(_)) => {
                    let wal_event = self.wal_next.take().expect("WAL event exists");
                    if self
                        .archived
                        .covers(wal_event.event.signal, wal_event.cursor)
                    {
                        continue;
                    }
                    if !self.repair_segments {
                        bail!(
                            "WAL cursor {} was not recovered into a segment",
                            wal_event.cursor
                        );
                    }
                    self.storage
                        .append(&wal_event)
                        .context("recover committed WAL event into a segment")?;
                    return Ok(Some(wal_event));
                }
                (Some(segment_cursor), None) => {
                    let segment = self.segment_next.take().expect("segment event exists");
                    if !self.archived.covers(segment.event.signal, segment_cursor) {
                        bail!(
                            "segment cursor {segment_cursor} has no committed WAL or archive receipt"
                        );
                    }
                    return Ok(Some(segment));
                }
            }
        }
    }
}
