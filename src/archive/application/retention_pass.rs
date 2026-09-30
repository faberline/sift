//! One bounded retention pass: the catalog root and manifest totals it carries
//! from the committed manifest to the next one, the spill catalogs of obsolete
//! and live objects, and the steps that expire events, blobs and dedupe
//! receipts through them.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::archive::application::retention_catalog_ops::{
    apply_catalog_remove, apply_catalog_upsert, retained_watermarks_from_catalog,
};
use crate::archive::domain::archive_catalog_item::{blob_catalog_key, ArchiveCatalogItem};
use crate::archive::domain::archive_event_time::acknowledgement_time_unix_nano;
use crate::archive::domain::archive_manifest::{
    ArchiveManifest, ArchiveRetentionDelta, ArchiveRetentionScan, ARCHIVE_FORMAT_VERSION,
};
use crate::archive::domain::archive_manifest_validator::validate_dedupe_receipt;
use crate::archive::domain::event_set_digest::{
    decode_event_content_digest, decode_event_id_digest, include_event_content, include_event_id,
};
use crate::archive::domain::retention_policy::RETENTION_SCAN_BATCH_SEGMENTS;
use crate::archive::infrastructure::archive_catalog::{
    catalog_for_uri, decode_archive_catalog_entry,
};
use crate::archive::infrastructure::archive_commit_state::ArchiveCommitState;
use crate::archive::infrastructure::gc_plan_builder::spill_pending_archive_gc;
use crate::archive::infrastructure::gcs_uri::{gcs_uri, split_gcs_uri};
use crate::archive::infrastructure::spill_catalog::SpillCatalog;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::shared_kernel::stored_event::StoredEvent;

/// What one bounded retention pass carries from the committed manifest to the
/// next one: the catalog root it edits, the objects it finds obsolete or still
/// live, and the manifest totals and digests it keeps up to date.
pub(super) struct RetentionPass {
    pub(super) catalog_root: storage_segment::CatalogRoot,
    pub(super) gc_candidates: SpillCatalog,
    pub(super) live_objects: SpillCatalog,
    event_count: u64,
    event_id_digest: [u8; 32],
    event_content_digest: [u8; 32],
    pub(super) segment_count: u64,
    blob_count: u64,
    pub(super) dedupe_receipt_count: u64,
    blob_decrements: BTreeMap<String, u64>,
    pub(super) expired_events: u64,
    pub(super) replaced_segments: usize,
    pub(super) removed_segments: usize,
}

/// Where a pass leaves the retention scan: the watermarks of what the catalog
/// still holds, the scan to resume if it did not finish, and the oldest
/// retained event time for the next manifest.
pub(super) struct FinalizedRetentionScan {
    retained_watermarks: ArchiveWatermarks,
    retention_scan: Option<ArchiveRetentionScan>,
    oldest_event_time_unix_nano: Option<i64>,
}

impl RetentionPass {
    /// Start a pass from the committed manifest. The obsolete-object spill
    /// starts with the pending local GC, the committed manifest and the
    /// objects its GC plan lists.
    pub(super) fn new(
        root: &Path,
        current_state: &ArchiveCommitState,
        current: &ArchiveManifest,
        spill_parent: &Path,
    ) -> Result<Self> {
        let catalog_root = current.catalog_root.clone();
        let mut gc_candidates = SpillCatalog::new(spill_parent, "retention-obsolete-")?;
        let live_objects = SpillCatalog::new(spill_parent, "retention-live-")?;
        seed_gc_candidates(root, current_state, current, &mut gc_candidates)?;
        Ok(Self {
            catalog_root,
            gc_candidates,
            live_objects,
            event_count: current.event_count,
            event_id_digest: decode_event_id_digest(&current.event_id_sha256)?,
            event_content_digest: decode_event_content_digest(&current.event_content_sha256)?,
            segment_count: current.segment_count,
            blob_count: current.blob_count,
            dedupe_receipt_count: current.dedupe_receipt_count,
            blob_decrements: BTreeMap::new(),
            expired_events: 0,
            replaced_segments: 0,
            removed_segments: 0,
        })
    }

    /// Split a fetched segment's events at the cutoff. Each expired event
    /// leaves the event count and digests and adds a decrement to every blob
    /// it references. Returns the retained events, how many expired, and
    /// whether an expired event's dedupe receipt is still inside the
    /// idempotency window.
    pub(super) fn partition_expired_events(
        &mut self,
        events: Vec<StoredEvent>,
        cutoff: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(Vec<StoredEvent>, u64, bool)> {
        let mut retained = Vec::with_capacity(events.len());
        let mut segment_expired = 0_u64;
        let receipt_cutoff_nanos = (now
            - chrono::Duration::seconds(
                crate::journal::domain::idempotency_window::IDEMPOTENCY_WINDOW_SECONDS,
            ))
        .timestamp_nanos_opt()
        .context("dedupe receipt retention cutoff is outside the nanosecond range")?;
        let mut has_active_expired_receipt = false;
        for event in events {
            let occurred = DateTime::parse_from_rfc3339(&event.event.occurred_at)
                .context("archive event occurred_at must be RFC3339")?
                .with_timezone(&Utc);
            if occurred < cutoff {
                segment_expired = segment_expired.saturating_add(1);
                has_active_expired_receipt |=
                    acknowledgement_time_unix_nano(&event)? >= receipt_cutoff_nanos;
                self.event_count = self
                    .event_count
                    .checked_sub(1)
                    .context("expired archive event count underflow")?;
                include_event_id(&mut self.event_id_digest, &event.event.event_id);
                include_event_content(&mut self.event_content_digest, &event)?;
                for reference in &event.event.blob_refs {
                    *self
                        .blob_decrements
                        .entry(reference.hash.clone())
                        .or_default() += 1;
                }
            } else {
                retained.push(event);
            }
        }
        Ok((retained, segment_expired, has_active_expired_receipt))
    }

    /// Apply the pass's blob decrements to the catalog. A blob whose reference
    /// count reaches zero leaves the catalog and becomes obsolete.
    pub(super) fn apply_blob_decrements(
        &mut self,
        current_catalog: &storage_segment::PagedCatalog,
        bucket: &str,
    ) -> Result<()> {
        for (hash, &decrement) in &self.blob_decrements {
            let key = format!("blob/{hash}");
            let entry = current_catalog
                .lookup(&self.catalog_root, &key)?
                .context("expired event references a blob absent from the archive catalog")?;
            let ArchiveCatalogItem::Blob(mut blob) = decode_archive_catalog_entry(entry)? else {
                bail!("archive blob key resolved to a segment");
            };
            if decrement > blob.reference_count {
                bail!("archive blob reference count underflow");
            }
            apply_catalog_remove(
                current_catalog,
                &mut self.catalog_root,
                &key,
                bucket,
                &mut self.gc_candidates,
                &mut self.live_objects,
            )?;
            blob.reference_count -= decrement;
            if blob.reference_count == 0 {
                self.blob_count = self
                    .blob_count
                    .checked_sub(1)
                    .context("retained blob count underflow")?;
                self.gc_candidates.insert_uri(&blob.object_uri)?;
            } else {
                apply_catalog_upsert(
                    current_catalog,
                    &mut self.catalog_root,
                    storage_segment::CatalogEntry {
                        key: blob_catalog_key(&blob),
                        value: serde_json::to_vec(&blob)?,
                    },
                    bucket,
                    &mut self.gc_candidates,
                    &mut self.live_objects,
                )?;
                self.live_objects.insert_uri(&blob.object_uri)?;
            }
        }
        Ok(())
    }

    /// Remove the dedupe receipts acknowledged entirely before the receipt
    /// cutoff, at most one scan batch of them per pass.
    pub(super) fn expire_dedupe_receipts(
        &mut self,
        current_catalog: &storage_segment::PagedCatalog,
        bucket: &str,
        receipt_cutoff_nanos: i64,
    ) -> Result<()> {
        let mut receipt_reader = current_catalog.reader_after(&self.catalog_root, "receipt/")?;
        let mut removed_receipts = 0_usize;
        while removed_receipts < RETENTION_SCAN_BATCH_SEGMENTS {
            let Some(entry) = receipt_reader.next() else {
                break;
            };
            let entry = entry?;
            if !entry.key.starts_with("receipt/") {
                break;
            }
            let ArchiveCatalogItem::Receipt(receipt) = decode_archive_catalog_entry(entry.clone())?
            else {
                bail!("archive receipt key resolved to another catalog item");
            };
            validate_dedupe_receipt(&receipt)?;
            if receipt.max_acknowledged_at_unix_nano >= receipt_cutoff_nanos {
                break;
            }
            apply_catalog_remove(
                current_catalog,
                &mut self.catalog_root,
                &entry.key,
                bucket,
                &mut self.gc_candidates,
                &mut self.live_objects,
            )?;
            self.gc_candidates.insert_uri(&receipt.object_uri)?;
            self.dedupe_receipt_count = self
                .dedupe_receipt_count
                .checked_sub(1)
                .context("archive dedupe receipt count underflow")?;
            removed_receipts += 1;
        }
        Ok(())
    }

    /// Check the catalog against the pass's totals, and settle the scan: done,
    /// or kept to resume after the last key this pass processed.
    pub(super) fn finalize_retention_scan(
        &self,
        current_catalog: &storage_segment::PagedCatalog,
        scan: &ArchiveRetentionScan,
        scan_complete: bool,
        current_oldest_event_time_unix_nano: Option<i64>,
    ) -> Result<FinalizedRetentionScan> {
        if self.catalog_root.entry_count
            != self
                .segment_count
                .saturating_add(self.blob_count)
                .saturating_add(self.dedupe_receipt_count)
        {
            bail!("bounded retention catalog count disagrees with its manifest totals");
        }
        let retained_watermarks =
            retained_watermarks_from_catalog(current_catalog, &self.catalog_root)?;
        let retention_scan = if scan_complete {
            if scan
                .oldest_retained_event_time_unix_nano
                .is_some_and(|oldest| oldest < scan.cutoff_unix_nano)
            {
                bail!("bounded retention completed but retained an expired segment minimum");
            }
            None
        } else {
            Some(scan.clone())
        };
        let oldest_event_time_unix_nano = retention_scan
            .as_ref()
            .and(current_oldest_event_time_unix_nano)
            .or(scan.oldest_retained_event_time_unix_nano);
        Ok(FinalizedRetentionScan {
            retained_watermarks,
            retention_scan,
            oldest_event_time_unix_nano,
        })
    }

    /// The manifest this pass commits: the committed manifest's source and
    /// history, the pass's totals, digests and catalog root, its GC plan, and
    /// a retention delta back to the committed manifest. It borrows the pass
    /// and clones the catalog root, so the spill directories stay until the
    /// caller returns.
    pub(super) fn build_retention_manifest(
        &self,
        now: DateTime<Utc>,
        current: &ArchiveManifest,
        current_state: &ArchiveCommitState,
        target_generation: u64,
        scan: &ArchiveRetentionScan,
        finalized: FinalizedRetentionScan,
        gc_plan: Option<storage_segment::StreamingCatalogBuild>,
        gc_plan_uri: Option<String>,
    ) -> ArchiveManifest {
        let gc_object_count = gc_plan
            .as_ref()
            .map(|plan| plan.root.entry_count)
            .unwrap_or_default();
        ArchiveManifest {
            format_version: ARCHIVE_FORMAT_VERSION,
            generated_at: now.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
            source_cluster_id: current.source_cluster_id.clone(),
            source_node_id: current.source_node_id.clone(),
            raft_snapshot_index: current.raft_snapshot_index,
            event_count: self.event_count,
            event_id_digest_algorithm: "xor-sha256-v1".to_string(),
            event_id_sha256: hex::encode(self.event_id_digest),
            event_content_digest_algorithm: "xor-sha256-v1".to_string(),
            event_content_sha256: hex::encode(self.event_content_digest),
            retention_generation: target_generation,
            watermarks: current.watermarks,
            retained_watermarks: finalized.retained_watermarks,
            oldest_event_time_unix_nano: finalized.oldest_event_time_unix_nano,
            epochs: current.epochs.clone(),
            catalog_uri: current.catalog_uri.clone(),
            catalog_root: self.catalog_root.clone(),
            segment_count: self.segment_count,
            blob_count: self.blob_count,
            dedupe_receipt_count: self.dedupe_receipt_count,
            gc_plan_uri,
            gc_plan_root: gc_plan.map(|plan| plan.root),
            gc_object_count,
            retention_delta: Some(ArchiveRetentionDelta {
                source_manifest_uri: current_state.manifest_uri.clone(),
                source_manifest_sha256: current_state.manifest_sha256.clone(),
                source_generation: current.retention_generation,
                source_event_count: current.event_count,
                source_event_content_sha256: current.event_content_sha256.clone(),
                cutoff_unix_nano: scan.cutoff_unix_nano,
            }),
            retention_scan: finalized.retention_scan,
            segments: Vec::new(),
            blobs: Vec::new(),
            gc_object_uris: Vec::new(),
        }
    }
}

/// Seed the obsolete-object spill with the pending local GC, the committed
/// manifest and the objects its GC plan lists.
fn seed_gc_candidates(
    root: &Path,
    current_state: &ArchiveCommitState,
    current: &ArchiveManifest,
    gc_candidates: &mut SpillCatalog,
) -> Result<()> {
    spill_pending_archive_gc(root, gc_candidates)?;
    gc_candidates.insert_uri(&current_state.manifest_uri)?;
    if let (Some(gc_uri), Some(gc_root)) = (&current.gc_plan_uri, &current.gc_plan_root) {
        let (gc_bucket, _) = split_gcs_uri(gc_uri)?;
        let current_gc = catalog_for_uri(gc_uri)?;
        for key in current_gc.page_keys(gc_root)? {
            gc_candidates.insert_uri(&gcs_uri(&gc_bucket, &key?))?;
        }
    }
    Ok(())
}
