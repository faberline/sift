//! Replaying the recent committed events and dedupe receipts into the journal.

use crate::archive::application::archive_services::ArchiveServices;

use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::archive::domain::archive_catalog_item::ArchiveCatalogItem;
use crate::archive::domain::archive_event_time::acknowledgement_time_unix_nano;
use crate::archive::domain::catalog_decode::decode_archive_catalog_entry;
use crate::shared_kernel::archive_receipts::ArchiveReplay;
use crate::shared_kernel::dedupe_receipt::DedupeReceipt;
use crate::shared_kernel::stored_event::StoredEvent;

/// Replay only archive rows that can still be inside the idempotency window.
/// Segment acceptance-time bounds avoid downloading the 180-day cold archive
/// when a rebuildable dedupe index is lost.
pub(crate) fn replay_recent_committed_events<F>(
    services: &ArchiveServices,
    root: &Path,
    since: DateTime<Utc>,
    mut visitor: F,
) -> Result<Option<ArchiveReplay>>
where
    F: FnMut(StoredEvent) -> Result<()>,
{
    let Some(manifest) = services.fetch_verified_committed_root(root)? else {
        return Ok(None);
    };
    let cutoff_nanos = since
        .timestamp_nanos_opt()
        .context("dedupe rebuild cutoff is outside the nanosecond range")?;
    let catalog = services.resources.catalog_for_uri(&manifest.catalog_uri)?;
    let mut scanned = 0_u64;
    let mut replayed = 0_u64;
    for entry in catalog.reader(&manifest.catalog_root)? {
        let ArchiveCatalogItem::Segment(segment) = decode_archive_catalog_entry(entry?)? else {
            continue;
        };
        if segment.min_acknowledged_at_unix_nano > segment.max_acknowledged_at_unix_nano {
            bail!("archive segment has invalid acknowledgement-time bounds");
        }
        if segment.max_acknowledged_at_unix_nano < cutoff_nanos {
            continue;
        }
        let bytes = services.cached_segment_bytes(root, &segment)?;
        let events = services.decode_parquet(&bytes)?;
        services.verify_archive_segment(&segment, &events)?;
        for event in events {
            scanned = scanned.saturating_add(1);
            if acknowledgement_time_unix_nano(&event)? < cutoff_nanos {
                continue;
            }
            visitor(event)?;
            replayed = replayed.saturating_add(1);
        }
    }
    Ok(Some(ArchiveReplay {
        watermark: manifest.raft_snapshot_index,
        scanned,
        replayed,
    }))
}

pub(crate) fn replay_recent_committed_receipts<F>(
    services: &ArchiveServices,
    root: &Path,
    since: DateTime<Utc>,
    mut visitor: F,
) -> Result<Option<u64>>
where
    F: FnMut(DedupeReceipt) -> Result<()>,
{
    let Some(manifest) = services.fetch_verified_committed_root(root)? else {
        return Ok(None);
    };
    let cutoff_nanos = since
        .timestamp_nanos_opt()
        .context("dedupe receipt rebuild cutoff is outside the nanosecond range")?;
    let catalog = services.resources.catalog_for_uri(&manifest.catalog_uri)?;
    let mut replayed = 0_u64;
    for entry in catalog.reader_after(&manifest.catalog_root, "receipt/")? {
        let entry = entry?;
        if !entry.key.starts_with("receipt/") {
            break;
        }
        let ArchiveCatalogItem::Receipt(receipt) = decode_archive_catalog_entry(entry)? else {
            bail!("archive receipt key resolved to another catalog item");
        };
        if receipt.max_acknowledged_at_unix_nano < cutoff_nanos {
            continue;
        }
        for row in services.fetch_dedupe_receipts(&receipt)? {
            if row.acknowledged_at_unix_nano < cutoff_nanos {
                continue;
            }
            visitor(row)?;
            replayed = replayed.saturating_add(1);
        }
    }
    Ok(Some(replayed))
}
