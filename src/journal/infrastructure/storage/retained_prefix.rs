//! Reconciling the locally retained segment prefix with the archive, and
//! checking that the retained copy is exact.

use anyhow::Result;

use crate::journal::domain::retained_prefix_reconcile_stats::RetainedPrefixReconcileStats;
use crate::journal::infrastructure::storage::raw_storage::RawStorage;
use crate::journal::infrastructure::storage::segment_read::SegmentEventReader;
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;

impl RawStorage {
    /// Replace the local archived prefix with the exact retained rows from a
    /// hash-verified restore. Rows after `snapshot_index` are a Raft suffix and
    /// remain untouched.
    #[doc(hidden)]
    pub fn reconcile_retained_prefix(
        &self,
        retained: &RawStorage,
        snapshot_index: u64,
    ) -> Result<RetainedPrefixReconcileStats> {
        let mut stats = RetainedPrefixReconcileStats::default();

        // First remove every local row in the authoritative checkpoint
        // prefix. Keep only the post-checkpoint Raft suffix. One immutable
        // segment is materialized at a time, so memory does not grow with the
        // total retained row count.
        for (signal, manifest) in self.seal_all_with_signal()? {
            let events = self.read_segment_events(signal, &manifest)?;
            stats.max_buffered_events = stats.max_buffered_events.max(events.len());
            let replacement = events
                .iter()
                .filter(|event| event.cursor > snapshot_index)
                .cloned()
                .collect::<Vec<_>>();
            if replacement == events {
                continue;
            }
            if !replacement.is_empty() {
                self.segments
                    .for_signal(signal)?
                    .write_reconciled_segment(&manifest.segment_id, &replacement)?;
            }
            self.evict_segment(signal, &manifest.segment_id)?;
        }

        // Rebuild the exact retained prefix from the verified restore source.
        // The source reader owns one frame at a time. The append batch is also
        // bounded by item count and encoded bytes.
        for signal in SignalKind::ALL {
            let mut reader = retained.segments.for_signal(signal)?.reader(0)?;
            let mut batch = Vec::with_capacity(1_000);
            let mut batch_bytes = 0_usize;
            while let Some(event) = reader.next_event()? {
                if event.event.signal != signal {
                    anyhow::bail!("retained archive contains the wrong signal");
                }
                if event.cursor > snapshot_index {
                    break;
                }
                let encoded = serde_json::to_vec(&event)?.len();
                if !batch.is_empty()
                    && (batch.len() == 1_000
                        || batch_bytes.saturating_add(encoded) > 16 * 1024 * 1024)
                {
                    stats.max_buffered_events = stats.max_buffered_events.max(batch.len());
                    self.append_batch(&batch)?;
                    batch.clear();
                    batch_bytes = 0;
                }
                batch_bytes = batch_bytes.saturating_add(encoded);
                batch.push(event);
            }
            if !batch.is_empty() {
                stats.max_buffered_events = stats.max_buffered_events.max(batch.len());
                self.append_batch(&batch)?;
            }
        }

        verify_local_retained_exact(self, retained, snapshot_index)?;
        Ok(stats)
    }
}

fn verify_local_retained_exact(
    local: &RawStorage,
    retained: &RawStorage,
    snapshot_index: u64,
) -> Result<()> {
    for signal in SignalKind::ALL {
        let mut local_reader = local.segments.for_signal(signal)?.reader(0)?;
        let mut retained_reader = retained.segments.for_signal(signal)?.reader(0)?;
        loop {
            let local_event = next_prefix_event(&mut local_reader, signal, snapshot_index)?;
            let retained_event = next_prefix_event(&mut retained_reader, signal, snapshot_index)?;
            if local_event != retained_event {
                anyhow::bail!(
                    "local {} prefix does not equal the retained archive through cursor {snapshot_index}",
                    signal
                );
            }
            if local_event.is_none() {
                break;
            }
        }
    }
    Ok(())
}

fn next_prefix_event(
    reader: &mut SegmentEventReader,
    signal: SignalKind,
    snapshot_index: u64,
) -> Result<Option<StoredEvent>> {
    let Some(event) = reader.next_event()? else {
        return Ok(None);
    };
    if event.event.signal != signal {
        anyhow::bail!("retained-prefix verification found the wrong signal");
    }
    Ok((event.cursor <= snapshot_index).then_some(event))
}
