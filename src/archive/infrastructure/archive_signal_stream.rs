//! Streaming one signal's archived events in cursor order from the catalog.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use parquet::arrow::arrow_reader::ParquetRecordBatchReader;

use crate::archive::domain::archive_catalog_item::ArchiveCatalogItem;
use crate::archive::infrastructure::archive_catalog::decode_archive_catalog_entry;
use crate::archive::infrastructure::archive_integrity::verify_archive_segment_file;
use crate::archive::infrastructure::archive_segment_cache::cached_segment_path;
use crate::archive::infrastructure::parquet_event_codec::{
    decode_parquet_batch, open_parquet_reader,
};
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;

pub(in crate::archive) struct ArchiveSignalStream {
    root: PathBuf,
    catalog: storage_segment::CatalogReader,
    prefix: String,
    pub(in crate::archive) signal: SignalKind,
    events: VecDeque<StoredEvent>,
    parquet: Option<ParquetRecordBatchReader>,
    last_cursor: Option<u64>,
    finished: bool,
}

impl ArchiveSignalStream {
    pub(in crate::archive) fn new(
        root: &Path,
        catalog: storage_segment::CatalogReader,
        signal: SignalKind,
    ) -> Self {
        Self::new_after(root, catalog, signal, 0)
    }

    pub(in crate::archive) fn new_after(
        root: &Path,
        catalog: storage_segment::CatalogReader,
        signal: SignalKind,
        after: u64,
    ) -> Self {
        Self {
            root: root.to_path_buf(),
            catalog,
            prefix: format!("segment/{signal}/"),
            signal,
            events: VecDeque::new(),
            parquet: None,
            last_cursor: Some(after),
            finished: false,
        }
    }

    pub(in crate::archive) fn peek_cursor(&mut self) -> Result<Option<u64>> {
        self.fill()?;
        Ok(self.events.front().map(|event| event.cursor))
    }

    pub(in crate::archive) fn pop_event(&mut self) -> Result<Option<StoredEvent>> {
        self.fill()?;
        let event = self.events.pop_front();
        if let Some(event) = &event {
            if self
                .last_cursor
                .is_some_and(|previous| event.cursor <= previous)
            {
                bail!("archive signal stream cursors are not strictly increasing");
            }
            self.last_cursor = Some(event.cursor);
        }
        Ok(event)
    }

    fn fill(&mut self) -> Result<()> {
        while self.events.is_empty() && !self.finished {
            if let Some(reader) = self.parquet.as_mut() {
                match reader.next() {
                    Some(batch) => {
                        let after = self.last_cursor.unwrap_or_default();
                        self.events = decode_parquet_batch(&batch?)?
                            .into_iter()
                            .filter(|event| event.cursor > after)
                            .collect();
                        continue;
                    }
                    None => {
                        self.parquet = None;
                        continue;
                    }
                }
            }
            let Some(entry) = self.catalog.next() else {
                self.finished = true;
                break;
            };
            let entry = entry?;
            if !entry.key.starts_with(&self.prefix) {
                self.finished = true;
                break;
            }
            let ArchiveCatalogItem::Segment(segment) = decode_archive_catalog_entry(entry)? else {
                bail!("archive segment prefix resolved to another catalog item");
            };
            if segment.signal != self.signal {
                bail!("archive segment prefix contains the wrong signal");
            }
            if segment.source.last_cursor <= self.last_cursor.unwrap_or_default() {
                continue;
            }
            let path = cached_segment_path(&self.root, &segment)?;
            verify_archive_segment_file(&segment, &path)?;
            self.parquet = Some(open_parquet_reader(&path)?);
        }
        Ok(())
    }
}
