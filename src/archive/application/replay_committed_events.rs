//! Replaying committed archive events for a project, environment and time
//! range.

use crate::archive::application::archive_services::ArchiveServices;

use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::archive::application::committed_event_reader::CommittedEventReader;
use crate::archive::domain::archive_catalog_item::ArchiveCatalogItem;
use crate::archive::domain::archive_event_time::parse_optional_archive_time;
use crate::archive::domain::catalog_decode::decode_archive_catalog_entry;
use crate::shared_kernel::archive_receipts::ArchiveReplay;
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;

/// Replay matching events from the latest committed remote manifest.
///
/// Parquet objects are hash-checked before use. A good local cache is reused,
/// but Sift still verifies the remote commit manifest on each cold query. This
/// makes a GCS outage visible instead of returning a silent empty result.
pub(crate) fn replay_committed_events<F>(
    services: &ArchiveServices,
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
    let Some(manifest) = services.fetch_verified_committed_root(root)? else {
        return Ok(None);
    };
    let start = parse_optional_archive_time("start", start)?;
    let end = parse_optional_archive_time("end", end)?;
    if start.zip(end).is_some_and(|(start, end)| start >= end) {
        bail!("archive query start must be earlier than end");
    }

    let mut scanned = 0_u64;
    let mut replayed = 0_u64;
    let (mut source_rows, mut source_bytes, mut entries) = (0u64, 0u64, 0usize);
    let catalog = services.resources.catalog_for_uri(&manifest.catalog_uri)?;
    for entry in catalog.reader(&manifest.catalog_root)? {
        entries += 1;
        if entries > 100_000 {
            bail!("query_limit: archive catalog scan exceeds 100000 entries");
        }
        let ArchiveCatalogItem::Segment(segment) = decode_archive_catalog_entry(entry?)? else {
            continue;
        };
        if segment.signal != signal {
            continue;
        }
        let min = segment.source.min_event_time_unix_nano;
        let max = segment.source.max_event_time_unix_nano;
        if min != 0
            && max >= min
            && (start.is_some_and(|t| t.timestamp_nanos_opt().is_some_and(|t| max < t))
                || end.is_some_and(|t| t.timestamp_nanos_opt().is_some_and(|t| min >= t)))
        {
            continue;
        }
        source_rows = source_rows
            .checked_add(segment.source.event_count)
            .context("archive row overflow")?;
        source_bytes = source_bytes
            .checked_add(segment.parquet_bytes)
            .context("archive byte overflow")?;
        if source_rows > 250_000 || source_bytes > 64 * 1024 * 1024 {
            bail!("query_limit: archive scan exceeds 250000 rows or 64 MiB; narrow the window");
        }
        let bytes = services.cached_segment_bytes(root, &segment)?;
        let events = services.decode_parquet(&bytes)?;
        services.verify_archive_segment(&segment, &events)?;
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
    services: &ArchiveServices,
    root: &Path,
    after: u64,
    limit: usize,
) -> Result<Option<Vec<StoredEvent>>> {
    if limit == 0 {
        return Ok(Some(Vec::new()));
    }
    let Some(mut reader) = CommittedEventReader::open_with_services(services, root, after)? else {
        return Ok(None);
    };
    reader.read_next(limit).map(Some)
}
