//! Replaying committed archive events for a project, environment and time
//! range.

use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::archive::application::archive_receipts::ArchiveReplay;
use crate::archive::application::committed_event_reader::CommittedEventReader;
use crate::archive::domain::archive_catalog_item::ArchiveCatalogItem;
use crate::archive::domain::archive_event_time::parse_optional_archive_time;
use crate::archive::infrastructure::archive_catalog::{
    catalog_for_uri, decode_archive_catalog_entry,
};
use crate::archive::infrastructure::archive_integrity::verify_archive_segment;
use crate::archive::infrastructure::archive_segment_cache::cached_segment_bytes;
use crate::archive::infrastructure::parquet_event_codec::decode_parquet;
use crate::archive::infrastructure::verified_manifest_fetcher::fetch_verified_committed_root;
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;

/// Replay matching events from the latest committed remote manifest.
///
/// Parquet objects are hash-checked before use. A good local cache is reused,
/// but Sift still verifies the remote commit manifest on each cold query. This
/// makes a GCS outage visible instead of returning a silent empty result.
pub fn replay_committed_events<F>(
    root: &Path,
    signal: SignalKind,
    project: &str,
    environment: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
    mut visitor: F,
) -> Result<Option<ArchiveReplay>>
where
    F: FnMut(StoredEvent) -> Result<()>,
{
    let Some(manifest) = fetch_verified_committed_root(root)? else {
        return Ok(None);
    };
    let start = parse_optional_archive_time("start", start)?;
    let end = parse_optional_archive_time("end", end)?;
    if start.zip(end).is_some_and(|(start, end)| start >= end) {
        bail!("archive query start must be earlier than end");
    }

    let mut scanned = 0_u64;
    let mut replayed = 0_u64;
    let catalog = catalog_for_uri(&manifest.catalog_uri)?;
    for entry in catalog.reader(&manifest.catalog_root)? {
        let ArchiveCatalogItem::Segment(segment) = decode_archive_catalog_entry(entry?)? else {
            continue;
        };
        if segment.signal != signal {
            continue;
        }
        let bytes = cached_segment_bytes(root, &segment)?;
        let events = decode_parquet(&bytes)?;
        verify_archive_segment(&segment, &events)?;
        for stored in events {
            scanned = scanned.saturating_add(1);
            let event = &stored.event;
            if event.project != project
                || environment.is_some_and(|environment| event.environment != environment)
            {
                continue;
            }
            let occurred = DateTime::parse_from_rfc3339(&event.occurred_at)
                .context("archive event occurred_at must be RFC3339")?
                .with_timezone(&Utc);
            if start.is_some_and(|start| occurred < start) || end.is_some_and(|end| occurred >= end)
            {
                continue;
            }
            visitor(stored)?;
            replayed = replayed.saturating_add(1);
        }
    }
    Ok(Some(ArchiveReplay {
        watermark: manifest.watermarks.through(signal),
        scanned,
        replayed,
    }))
}

/// Read one globally ordered page from the committed retained archive.
///
/// Projection rebuilds use this path after retention changes. It joins the
/// three signal streams by raw cursor and never treats an unavailable archive
/// as an empty source.
pub(crate) fn read_committed_events_after(
    root: &Path,
    after: u64,
    limit: usize,
) -> Result<Option<Vec<StoredEvent>>> {
    if limit == 0 {
        return Ok(Some(Vec::new()));
    }
    let Some(mut reader) = CommittedEventReader::open(root, after)? else {
        return Ok(None);
    };
    reader.read_next(limit).map(Some)
}
