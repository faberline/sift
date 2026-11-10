//! A projection's read session over the journal: resident events first, the
//! archive behind them.

use std::sync::Arc;

use anyhow::{bail, Result};

use crate::journal::application::journal_runtime::JournalRuntime;
use crate::journal::domain::event_store::EventReader;
use crate::shared_kernel::stored_event::StoredEvent;

#[doc(hidden)]
pub struct RuntimeReadSession {
    journal: Arc<JournalRuntime>,
    archive: Option<
        Box<dyn crate::journal::domain::archive_access::ArchivedEventReader<Error = anyhow::Error>>,
    >,
    archive_through: u64,
    cursor: u64,
    local: Option<Box<dyn EventReader<Error = anyhow::Error>>>,
}

impl RuntimeReadSession {
    #[doc(hidden)]
    pub fn read_next(&mut self, limit: usize) -> Result<Vec<StoredEvent>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut page = Vec::with_capacity(limit);
        let mut refreshed_at = None;
        while page.len() < limit {
            if let Some(archive) = self.archive.as_mut() {
                let remaining = limit - page.len();
                let archived = archive.read_next(remaining)?;
                if let Some(last) = archived.last() {
                    self.cursor = last.cursor;
                }
                let exhausted = archived.len() < remaining;
                page.extend(archived);
                if !exhausted {
                    return Ok(page);
                }
                self.archive = None;
                self.cursor = self.cursor.max(self.archive_through);
                continue;
            }
            if self.local.is_none() {
                if self.refresh_archive()? {
                    continue;
                }
                // Keep one streaming cursor. Do not preload an unrelated
                // 100,000-event suffix for a small query page.
                self.local = Some(self.journal.storage.reader(self.cursor)?);
            }
            match self
                .local
                .as_mut()
                .expect("local reader exists")
                .next_event()
            {
                Ok(Some(event)) => {
                    if event.cursor <= self.cursor {
                        bail!("projection source cursors are not strictly increasing");
                    }
                    self.cursor = event.cursor;
                    page.push(event);
                }
                Ok(None) => {
                    self.local = None;
                    if self.refresh_archive()? {
                        continue;
                    }
                    // A seal or append can replace paths after this reader
                    // opened. Refresh once at a cursor with a known suffix.
                    if self.cursor < self.journal.last_cursor() && refreshed_at != Some(self.cursor)
                    {
                        refreshed_at = Some(self.cursor);
                        continue;
                    }
                    break;
                }
                Err(error) => {
                    self.local = None;
                    if self.refresh_archive()? {
                        continue;
                    }
                    if refreshed_at != Some(self.cursor)
                        && error.chain().any(|cause| {
                            cause
                                .downcast_ref::<std::io::Error>()
                                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
                        })
                    {
                        refreshed_at = Some(self.cursor);
                        continue;
                    }
                    return Err(error);
                }
            }
        }
        Ok(page)
    }

    fn refresh_archive(&mut self) -> Result<bool> {
        let Some(reader) = self
            .journal
            .archive_read
            .open_reader(self.journal.data_dir(), self.cursor)?
        else {
            return Ok(false);
        };
        self.archive_through = reader.snapshot_index();
        self.archive = Some(reader);
        Ok(true)
    }
}

impl JournalRuntime {
    #[doc(hidden)]
    pub fn projection_read_session(self: Arc<Self>, after: u64) -> Result<RuntimeReadSession> {
        self.ensure_recovered()?;
        let archive = self.archive_read.open_reader(self.data_dir(), after)?;
        let archive_through = archive
            .as_ref()
            .map(|reader| reader.snapshot_index())
            .unwrap_or(after);
        Ok(RuntimeReadSession {
            journal: self.clone(),
            archive,
            archive_through,
            cursor: after,
            local: None,
        })
    }
}

impl crate::journal::domain::projection_reader::ProjectionReader for RuntimeReadSession {
    type Error = anyhow::Error;
    fn read_next(&mut self, limit: usize) -> Result<Vec<StoredEvent>> {
        RuntimeReadSession::read_next(self, limit)
    }
}
#[doc(hidden)]
pub struct JournalProjectionReadSession {
    reader:
        Box<dyn crate::journal::domain::projection_reader::ProjectionReader<Error = anyhow::Error>>,
}
impl JournalProjectionReadSession {
    pub fn read_next(&mut self, limit: usize) -> Result<Vec<StoredEvent>> {
        self.reader.read_next(limit)
    }
}
impl super::journal_service::JournalService<'static> {
    #[doc(hidden)]
    pub fn projection_read_session(
        self: &Arc<Self>,
        after: u64,
    ) -> Result<JournalProjectionReadSession> {
        Ok(JournalProjectionReadSession {
            reader: self.port().projection_read_session(after)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::application::journal_service::DurableJournal;
    use crate::{EventEnvelope, SignalKind};

    fn append(journal: &DurableJournal, id: &str) {
        let mut event = EventEnvelope::for_project(
            "reader-bounds",
            "test",
            id,
            SignalKind::Log,
            serde_json::json!({"message": id}),
        );
        event
            .resource
            .insert("service.name".into(), "reader-test".into());
        journal.append(event).unwrap();
    }

    #[test]
    fn a_small_page_does_not_decode_an_unrequested_later_event() {
        let root = tempfile::tempdir().unwrap();
        let journal = Arc::new(DurableJournal::open(root.path()).unwrap());
        append(&journal, "first");
        append(&journal, "later");
        let path = journal.storage().active_segment_paths().pop().unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let first_size = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let second = 16 + first_size;
        let last = bytes.len() - 1;
        bytes[last] = b'!';
        let checksum = crc32fast::hash(&bytes[second + 16..]).to_le_bytes();
        bytes[second + 12..second + 16].copy_from_slice(&checksum);
        std::fs::write(path, bytes).unwrap();
        let mut reader = journal.projection_read_session(0).unwrap();
        let first = reader.read_next(1).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].event.event_id, "first");
        assert!(reader.read_next(1).is_err());
    }

    #[test]
    fn a_session_can_read_new_local_files_after_the_previous_end() {
        let root = tempfile::tempdir().unwrap();
        let journal = Arc::new(DurableJournal::open(root.path()).unwrap());
        append(&journal, "first");
        let mut reader = journal.projection_read_session(0).unwrap();
        assert_eq!(reader.read_next(10).unwrap().len(), 1);
        assert!(reader.read_next(10).unwrap().is_empty());
        journal.storage().seal_all().unwrap();
        append(&journal, "second");
        let next = reader.read_next(10).unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].cursor, 2);
        assert_eq!(next[0].event.event_id, "second");
    }

    #[test]
    fn a_truncated_committed_local_suffix_is_not_reported_as_exhausted() {
        let root = tempfile::tempdir().unwrap();
        let journal = Arc::new(DurableJournal::open(root.path()).unwrap());
        append(&journal, "first");
        append(&journal, "missing");
        let path = journal.storage().active_segment_paths().pop().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let first_end = 16 + u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as u64;
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_len(first_end)
            .unwrap();
        let mut reader = journal.projection_read_session(0).unwrap();
        assert_eq!(reader.read_next(1).unwrap()[0].cursor, 1);
        assert!(reader.read_next(1).is_err());
    }
}
