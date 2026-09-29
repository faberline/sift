//! Evicting committed segments older than the hot window from local storage.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::archive::application::archive_receipts::HotEvictionReceipt;
use crate::archive::domain::archive_catalog_item::{segment_catalog_key, ArchiveCatalogItem};
use crate::archive::domain::archive_manifest::{empty_dedupe_receipt, ArchiveSegment};
use crate::archive::domain::portable_manifest::portable_manifest;
use crate::archive::infrastructure::archive_catalog::{
    catalog_for_uri, decode_archive_catalog_entry,
};
use crate::archive::infrastructure::verified_manifest_fetcher::fetch_verified_committed_root;

/// Evict local copies whose complete event set is older than the 30-day hot
/// window and is present in the verified remote manifest.
pub fn evict_committed_cold_segments_at(
    journal: &crate::journal::infrastructure::durable_journal::DurableJournal,
    now: DateTime<Utc>,
) -> Result<HotEvictionReceipt> {
    let manifest = fetch_verified_committed_root(journal.storage().root())?
        .context("cold segment eviction requires a committed remote manifest")?;
    let catalog = catalog_for_uri(&manifest.catalog_uri)?;
    let cutoff = now - chrono::Duration::days(30);
    let cutoff_nanos = cutoff
        .timestamp_nanos_opt()
        .context("30-day hot retention cutoff is outside the nanosecond range")?;
    let mut receipt = HotEvictionReceipt::default();
    for (signal, local) in journal.storage().seal_all_with_signal()? {
        if local.last_cursor > manifest.watermarks.through(signal) {
            // This immutable file also carries a newer Raft suffix. It cannot
            // be evicted until a later compaction splits or archives that
            // suffix.
            continue;
        }
        let events = journal.storage().read_segment_events(signal, &local)?;
        let entirely_cold = if local.max_event_time_unix_nano == 0 {
            events.iter().try_fold(true, |cold, event| {
                let occurred = DateTime::parse_from_rfc3339(&event.event.occurred_at)
                    .context("segment event occurred_at must be RFC3339")?
                    .with_timezone(&Utc);
                anyhow::Ok(cold && occurred < cutoff)
            })?
        } else {
            local.max_event_time_unix_nano < cutoff_nanos
        };
        if !entirely_cold {
            continue;
        }
        // Segment boundaries are local implementation details. A follower can
        // seal the same Raft rows at different points than the leader. Require
        // exact metadata when such a remote segment exists, otherwise rely on
        // the verified manifest coverage and the cold-time check above.
        let portable = portable_manifest(local.clone(), signal);
        let probe = ArchiveSegment {
            signal,
            source: portable.clone(),
            object_uri: String::new(),
            parquet_bytes: 0,
            parquet_sha256: String::new(),
            min_acknowledged_at_unix_nano: 0,
            max_acknowledged_at_unix_nano: 0,
            dedupe_receipt: empty_dedupe_receipt(),
        };
        let exact_remote = catalog
            .lookup(&manifest.catalog_root, &segment_catalog_key(&probe))?
            .map(decode_archive_catalog_entry)
            .transpose()?
            .is_some_and(|item| {
                matches!(item, ArchiveCatalogItem::Segment(segment) if segment.signal == signal && segment.source == portable)
            });
        if !exact_remote
            && events
                .iter()
                .any(|event| event.cursor > manifest.watermarks.through(signal))
        {
            bail!(
                "local segment {} extends beyond committed archive coverage",
                local.segment_id
            );
        }
        if journal
            .storage()
            .evict_segment(signal, &local.segment_id)?
            .is_some()
        {
            receipt.evicted_segments += 1;
            receipt.evicted_events = receipt.evicted_events.saturating_add(local.event_count);
            receipt.evicted_through_cursor = receipt.evicted_through_cursor.max(local.last_cursor);
        }
    }
    journal.evict_resident_before(cutoff)?;
    Ok(receipt)
}
