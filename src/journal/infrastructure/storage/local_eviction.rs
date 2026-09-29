//! Evicting expired and archived segments from local storage.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::journal::domain::segment_manifest::SegmentManifest;
use crate::journal::infrastructure::storage::raw_storage::RawStorage;
use crate::SignalKind;

impl RawStorage {
    /// Remove only local cache rows older than a committed retention cutoff.
    /// Rows after the archived Raft prefix stay intact. The remote manifest is
    /// still the authority for every cold row while a bounded scan continues.
    pub(crate) fn evict_expired_before(
        &self,
        cutoff: DateTime<Utc>,
        snapshot_index: u64,
    ) -> Result<u64> {
        let cutoff_nanos = cutoff
            .timestamp_nanos_opt()
            .context("local retention cutoff is outside the nanosecond range")?;
        let mut removed = 0_u64;
        for (signal, manifest) in self.seal_all_with_signal()? {
            if manifest.first_cursor > snapshot_index
                || manifest.min_event_time_unix_nano >= cutoff_nanos
            {
                continue;
            }
            let events = self.read_segment_events(signal, &manifest)?;
            let mut retained = Vec::with_capacity(events.len());
            for event in events {
                let occurred = DateTime::parse_from_rfc3339(&event.event.occurred_at)
                    .context("local retained event occurred_at must be RFC3339")?
                    .with_timezone(&Utc);
                if event.cursor <= snapshot_index && occurred < cutoff {
                    removed = removed.saturating_add(1);
                } else {
                    retained.push(event);
                }
            }
            if retained.len() as u64 == manifest.event_count {
                continue;
            }
            if !retained.is_empty() {
                self.segments
                    .for_signal(signal)?
                    .write_reconciled_segment(&manifest.segment_id, &retained)?;
            }
            self.evict_segment(signal, &manifest.segment_id)?;
        }
        Ok(removed)
    }

    pub(crate) fn evict_segment(
        &self,
        signal: SignalKind,
        segment_id: &str,
    ) -> Result<Option<SegmentManifest>> {
        let receipt_root = self
            .root
            .join("archive-cache")
            .join("evicted")
            .join(match signal {
                SignalKind::Log => "logs",
                SignalKind::Metric => "metrics",
                SignalKind::Span => "traces",
            });
        self.segments
            .for_signal(signal)?
            .evict_segment(segment_id, &receipt_root)
    }
}
