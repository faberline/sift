//! Reading the committed archive's events in cursor order.

use std::path::Path;

use anyhow::{Context, Result};

use crate::archive::application::archive_status_queries::committed_status;
use crate::archive::infrastructure::archive_catalog::catalog_for_uri;
use crate::archive::infrastructure::archive_signal_stream::ArchiveSignalStream;
use crate::archive::infrastructure::verified_manifest_fetcher::fetch_verified_committed_root;
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;

/// One linear, globally ordered scan of the committed retained archive.
///
/// The local commit receipt is checked before any remote request. A caller
/// whose cursor is already beyond the committed archive prefix gets `None` and
/// can continue from local WAL/segments during a GCS outage.
#[doc(hidden)]
pub struct CommittedEventReader {
    streams: [ArchiveSignalStream; 3],
    snapshot_index: u64,
}

impl CommittedEventReader {
    pub fn open(root: &Path, after: u64) -> Result<Option<Self>> {
        let Some(status) = committed_status(root)? else {
            return Ok(None);
        };
        if after >= status.snapshot_index {
            return Ok(None);
        }
        let manifest = fetch_verified_committed_root(root)?
            .context("committed archive receipt has no readable manifest")?;
        let catalog = catalog_for_uri(&manifest.catalog_uri)?;
        let stream = |signal| -> Result<ArchiveSignalStream> {
            let prefix = format!("segment/{signal}/");
            Ok(ArchiveSignalStream::new_after(
                root,
                catalog.reader_after(&manifest.catalog_root, &prefix)?,
                signal,
                after,
            ))
        };
        Ok(Some(Self {
            streams: [
                stream(SignalKind::Log)?,
                stream(SignalKind::Metric)?,
                stream(SignalKind::Span)?,
            ],
            snapshot_index: manifest.raft_snapshot_index,
        }))
    }

    pub fn snapshot_index(&self) -> u64 {
        self.snapshot_index
    }

    pub fn read_next(&mut self, limit: usize) -> Result<Vec<StoredEvent>> {
        let mut page = Vec::with_capacity(limit);
        while page.len() < limit {
            let mut next = None::<(usize, u64)>;
            for (index, stream) in self.streams.iter_mut().enumerate() {
                if let Some(cursor) = stream.peek_cursor()? {
                    if next.is_none_or(|(_, current)| cursor < current) {
                        next = Some((index, cursor));
                    }
                }
            }
            let Some((index, _)) = next else {
                break;
            };
            page.push(
                self.streams[index]
                    .pop_event()?
                    .context("archive projection stream head disappeared")?,
            );
        }
        Ok(page)
    }
}
