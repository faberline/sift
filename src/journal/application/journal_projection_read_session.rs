//! A projection's read session over the journal: resident events first, the
//! archive behind them.

use std::collections::VecDeque;
use std::sync::Arc;

use anyhow::{bail, Result};

use crate::journal::application::journal_runtime::JournalRuntime;
use crate::journal::domain::journal_limits::PROJECTION_LOCAL_BUFFER_EVENTS;
use crate::shared_kernel::event_query::EventQuery;
use crate::shared_kernel::stored_event::StoredEvent;

#[doc(hidden)]
pub struct RuntimeReadSession {
    journal: Arc<JournalRuntime>,
    archive: Option<
        Box<dyn crate::journal::domain::archive_access::ArchivedEventReader<Error = anyhow::Error>>,
    >,
    archive_through: u64,
    cursor: u64,
    local: VecDeque<StoredEvent>,
}

impl RuntimeReadSession {
    #[doc(hidden)]
    pub fn read_next(&mut self, limit: usize) -> Result<Vec<StoredEvent>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut page = Vec::with_capacity(limit);
        if let Some(archive) = self.archive.as_mut() {
            let archived = archive.read_next(limit)?;
            if let Some(last) = archived.last() {
                self.cursor = last.cursor;
            }
            let exhausted = archived.len() < limit;
            page.extend(archived);
            if !exhausted {
                return Ok(page);
            }
            self.archive = None;
            self.cursor = self.cursor.max(self.archive_through);
        }
        while page.len() < limit {
            if self.local.is_empty() {
                // A background archive can commit and evict the next local
                // suffix after this session opened. Refresh the manifest at
                // the current cursor before consulting local files.
                if self.refresh_archive()? {
                    let archived = self
                        .archive
                        .as_mut()
                        .expect("refreshed archive reader exists")
                        .read_next(limit - page.len())?;
                    if let Some(last) = archived.last() {
                        self.cursor = last.cursor;
                    }
                    let exhausted = archived.len() < limit - page.len();
                    page.extend(archived);
                    if !exhausted {
                        return Ok(page);
                    }
                    self.archive = None;
                    self.cursor = self.cursor.max(self.archive_through);
                    continue;
                }
                self.local = self
                    .journal
                    .query_local_unchecked(
                        EventQuery {
                            signal: None,
                            after: self.cursor,
                            limit: PROJECTION_LOCAL_BUFFER_EVENTS,
                        },
                        PROJECTION_LOCAL_BUFFER_EVENTS,
                    )?
                    .into();
                if self.local.is_empty() {
                    // Close the commit/eviction race. The first refresh can
                    // observe the old receipt immediately before local files
                    // are evicted under a new committed receipt.
                    if self.refresh_archive()? {
                        continue;
                    }
                    break;
                }
            }
            while page.len() < limit {
                let Some(event) = self.local.pop_front() else {
                    break;
                };
                if event.cursor <= self.cursor {
                    bail!("projection source cursors are not strictly increasing");
                }
                self.cursor = event.cursor;
                page.push(event);
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
            local: VecDeque::new(),
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
