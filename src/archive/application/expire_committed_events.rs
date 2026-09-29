//! Expiring committed events past the retention window in bounded passes.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::archive::application::archive_receipts::{ArchiveReceipt, ExpirationReceipt};
use crate::archive::application::reconcile_committed_retention::reconcile_live_committed_retention;
use crate::archive::application::retention_catalog_ops::{
    apply_catalog_remove, apply_catalog_upsert, retained_watermarks_from_catalog,
};
use crate::archive::application::retention_due::committed_has_expired_dedupe_receipt;
use crate::archive::application::retention_manifest_commit::{
    finish_bounded_retention_commit, validate_retention_receipt_source,
};
use crate::archive::domain::archive_catalog_item::{
    blob_catalog_key, dedupe_receipt_catalog_key, segment_catalog_key, ArchiveCatalogItem,
};
use crate::archive::domain::archive_event_time::{
    acknowledgement_time_bounds, acknowledgement_time_unix_nano, event_time_unix_nano,
};
use crate::archive::domain::archive_manifest::{
    ArchiveManifest, ArchiveRetentionDelta, ArchiveRetentionScan, ArchiveSegment,
    ARCHIVE_FORMAT_VERSION,
};
use crate::archive::domain::archive_manifest_validator::{
    validate_archive_manifest, validate_dedupe_receipt,
};
use crate::archive::domain::event_set_digest::{
    decode_event_content_digest, decode_event_id_digest, include_event_content, include_event_id,
};
use crate::archive::domain::retention_policy::{
    include_retention_oldest, RETENTION_SCAN_BATCH_SEGMENTS,
};
use crate::archive::infrastructure::archive_catalog::{
    catalog_for_uri, decode_archive_catalog_entry,
};
use crate::archive::infrastructure::archive_commit_state::read_commit_state;
use crate::archive::infrastructure::archive_integrity::{sha256, verify_archive_segment};
use crate::archive::infrastructure::archive_segment_cache::cached_segment_bytes;
use crate::archive::infrastructure::dedupe_receipt_archive::{
    archive_dedupe_receipts, fetch_dedupe_receipts,
};
use crate::archive::infrastructure::gc_plan_builder::{
    build_gc_catalog_from_spills, spill_pending_archive_gc,
};
use crate::archive::infrastructure::gcs_uri::{gcs_uri, split_gcs_uri};
use crate::archive::infrastructure::parquet_event_codec::{decode_parquet, encode_parquet};
use crate::archive::infrastructure::spill_catalog::SpillCatalog;
use crate::archive::infrastructure::verified_manifest_fetcher::fetch_verified_committed_root;

/// Remove events older than the fixed 180-day boundary from the committed
/// manifest. A mixed Parquet segment is rewritten with only retained rows.
/// The new objects and manifest commit before the prior objects are deleted.
pub fn expire_committed_events_at(
    journal: &crate::journal::infrastructure::durable_journal::DurableJournal,
    now: DateTime<Utc>,
) -> Result<ExpirationReceipt> {
    expire_committed_events_bounded_at(journal, now)
}

fn expire_committed_events_bounded_at(
    journal: &crate::journal::infrastructure::durable_journal::DurableJournal,
    now: DateTime<Utc>,
) -> Result<ExpirationReceipt> {
    const BATCH_PARQUET_BYTES: u64 = 64 * 1024 * 1024;

    reconcile_live_committed_retention(journal)?;
    let root = journal.storage().root();
    let current_state = read_commit_state(root)?
        .context("180-day expiration requires a committed remote manifest")?;
    let current = fetch_verified_committed_root(root)?
        .context("180-day expiration requires a readable remote manifest")?;
    if journal.last_cursor() < current.raft_snapshot_index {
        bail!(
            "180-day expiration journal cursor {} is behind archive cursor {}",
            journal.last_cursor(),
            current.raft_snapshot_index
        );
    }
    let suffix_events = journal.last_cursor() - current.raft_snapshot_index;
    let expected_local_events = current
        .event_count
        .checked_add(suffix_events)
        .context("retained archive and suffix event count exhausted u64")?;
    if journal.total_event_count() != expected_local_events
        || journal.recovery_required()
        || journal.retention_generation() != current.retention_generation
    {
        bail!("Sift journal must reconcile its committed retention head before advancing it");
    }

    let requested_cutoff_nanos = (now - chrono::Duration::days(180))
        .timestamp_nanos_opt()
        .context("180-day retention cutoff is outside the nanosecond range")?;
    let receipt_cutoff_nanos = (now
        - chrono::Duration::seconds(
            crate::journal::domain::idempotency_window::IDEMPOTENCY_WINDOW_SECONDS,
        ))
    .timestamp_nanos_opt()
    .context("dedupe receipt cleanup cutoff is outside the nanosecond range")?;
    if current.retention_scan.is_none()
        && current
            .oldest_event_time_unix_nano
            .is_none_or(|oldest| oldest >= requested_cutoff_nanos)
        && !committed_has_expired_dedupe_receipt(&current, receipt_cutoff_nanos)?
    {
        return Ok(ExpirationReceipt {
            manifest_uri: current_state.manifest_uri,
            retained_events: current.event_count,
            retained_segments: usize::try_from(current.segment_count).unwrap_or(usize::MAX),
            expired_events: 0,
            replaced_segments: 0,
            removed_segments: 0,
        });
    }

    let mut scan = current
        .retention_scan
        .clone()
        .unwrap_or(ArchiveRetentionScan {
            cutoff_unix_nano: requested_cutoff_nanos,
            after_catalog_key: "segment/".to_string(),
            oldest_retained_event_time_unix_nano: None,
        });
    let cutoff = DateTime::<Utc>::from_timestamp_nanos(scan.cutoff_unix_nano);
    let (bucket, current_manifest_key) = split_gcs_uri(&current_state.manifest_uri)?;
    let object_store: Arc<dyn storage_object::ObjectStore> =
        Arc::new(storage_object::GcsObjectStore::new(&bucket, "")?);
    let parent = current_manifest_key
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("sift");
    let target_generation = current.retention_generation.saturating_add(1);
    let rewrite_prefix = format!(
        "{parent}/retention-g{target_generation:020}-{}",
        &current_state.manifest_sha256[..16]
    );
    let manifest_key = format!("{rewrite_prefix}/manifest.json");
    match object_store.get(&manifest_key) {
        Ok(object) => {
            let manifest: ArchiveManifest = serde_json::from_slice(&object.bytes)
                .context("decode an uploaded bounded retention manifest")?;
            validate_archive_manifest(&manifest)?;
            let receipt = ArchiveReceipt {
                manifest_uri: gcs_uri(&bucket, &manifest_key),
                manifest_sha256: sha256(&object.bytes),
                manifest,
            };
            validate_retention_receipt_source(&receipt, &current_state)?;
            return finish_bounded_retention_commit(journal, &current, receipt);
        }
        Err(storage_object::ObjectStoreError::NotFound { .. }) => {}
        Err(error) => return Err(error.into()),
    }

    let current_catalog = catalog_for_uri(&current.catalog_uri)?;
    let mut catalog_root = current.catalog_root.clone();
    let mut reader =
        current_catalog.reader_after(&current.catalog_root, &scan.after_catalog_key)?;
    let spill_parent = root.join("tmp");
    let mut gc_candidates = SpillCatalog::new(&spill_parent, "retention-obsolete-")?;
    let mut live_objects = SpillCatalog::new(&spill_parent, "retention-live-")?;
    spill_pending_archive_gc(root, &mut gc_candidates)?;
    gc_candidates.insert_uri(&current_state.manifest_uri)?;
    if let (Some(gc_uri), Some(gc_root)) = (&current.gc_plan_uri, &current.gc_plan_root) {
        let (gc_bucket, _) = split_gcs_uri(gc_uri)?;
        let current_gc = catalog_for_uri(gc_uri)?;
        for key in current_gc.page_keys(gc_root)? {
            gc_candidates.insert_uri(&gcs_uri(&gc_bucket, &key?))?;
        }
    }

    let coordinator = storage_segment::ArchiveCoordinator::new(object_store.clone());
    let mut transaction = coordinator.begin();
    let mut event_count = current.event_count;
    let mut event_id_digest = decode_event_id_digest(&current.event_id_sha256)?;
    let mut event_content_digest = decode_event_content_digest(&current.event_content_sha256)?;
    let mut segment_count = current.segment_count;
    let mut blob_count = current.blob_count;
    let mut dedupe_receipt_count = current.dedupe_receipt_count;
    let mut blob_decrements = BTreeMap::<String, u64>::new();
    let mut expired_events = 0_u64;
    let mut replaced_segments = 0_usize;
    let mut removed_segments = 0_usize;
    let mut processed_segments = 0_usize;
    let mut fetched_parquet_bytes = 0_u64;
    let mut scan_complete = false;

    while processed_segments < RETENTION_SCAN_BATCH_SEGMENTS {
        let Some(entry) = reader.next() else {
            scan_complete = true;
            break;
        };
        let entry = entry?;
        let entry_key = entry.key.clone();
        let ArchiveCatalogItem::Segment(segment) = decode_archive_catalog_entry(entry)? else {
            scan.after_catalog_key = entry_key;
            continue;
        };
        let might_expire = segment.source.min_event_time_unix_nano < scan.cutoff_unix_nano;
        if might_expire
            && processed_segments > 0
            && fetched_parquet_bytes.saturating_add(segment.parquet_bytes) > BATCH_PARQUET_BYTES
        {
            break;
        }
        processed_segments += 1;
        scan.after_catalog_key = entry_key.clone();
        if !might_expire {
            include_retention_oldest(
                &mut scan.oldest_retained_event_time_unix_nano,
                segment.source.min_event_time_unix_nano,
            );
            continue;
        }

        fetched_parquet_bytes = fetched_parquet_bytes.saturating_add(segment.parquet_bytes);
        let bytes = cached_segment_bytes(root, &segment)?;
        let events = decode_parquet(&bytes)?;
        verify_archive_segment(&segment, &events)?;
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
                event_count = event_count
                    .checked_sub(1)
                    .context("expired archive event count underflow")?;
                include_event_id(&mut event_id_digest, &event.event.event_id);
                include_event_content(&mut event_content_digest, &event)?;
                for reference in &event.event.blob_refs {
                    *blob_decrements.entry(reference.hash.clone()).or_default() += 1;
                }
            } else {
                retained.push(event);
            }
        }
        if segment_expired == 0 {
            include_retention_oldest(
                &mut scan.oldest_retained_event_time_unix_nano,
                segment.source.min_event_time_unix_nano,
            );
            continue;
        }
        expired_events = expired_events.saturating_add(segment_expired);
        apply_catalog_remove(
            &current_catalog,
            &mut catalog_root,
            &entry_key,
            &bucket,
            &mut gc_candidates,
            &mut live_objects,
        )?;
        gc_candidates.insert_uri(&segment.object_uri)?;
        gc_candidates.insert_uri(&segment.dedupe_receipt.object_uri)?;
        segment_count = segment_count
            .checked_sub(1)
            .context("retained segment count underflow")?;
        if has_active_expired_receipt {
            fetch_dedupe_receipts(&segment.dedupe_receipt)
                .context("verify active dedupe receipt before retaining it")?;
            apply_catalog_upsert(
                &current_catalog,
                &mut catalog_root,
                storage_segment::CatalogEntry {
                    key: dedupe_receipt_catalog_key(&segment.dedupe_receipt),
                    value: serde_json::to_vec(&segment.dedupe_receipt)?,
                },
                &bucket,
                &mut gc_candidates,
                &mut live_objects,
            )?;
            live_objects.insert_uri(&segment.dedupe_receipt.object_uri)?;
            dedupe_receipt_count = dedupe_receipt_count.saturating_add(1);
        }
        if retained.is_empty() {
            removed_segments += 1;
            continue;
        }

        replaced_segments += 1;
        let parquet = encode_parquet(&retained)?;
        let parquet_sha256 = sha256(&parquet);
        let key = format!(
            "{rewrite_prefix}/segments/{}/{}.parquet",
            segment.signal, parquet_sha256
        );
        transaction.put(storage_segment::ArchiveObject::new(
            key.clone(),
            parquet.clone(),
            "application/vnd.apache.parquet",
        ))?;
        let mut source = segment.source.clone();
        source.segment_id = format!("retained-{}", &parquet_sha256[..32]);
        source.first_cursor = retained.first().expect("retained segment").cursor;
        source.last_cursor = retained.last().expect("retained segment").cursor;
        source.event_count = retained.len() as u64;
        source.bytes = parquet.len() as u64;
        source.sha256 = parquet_sha256.clone();
        source.local_path = PathBuf::from(format!(
            "segments/{}/{}.framed",
            segment.signal, source.segment_id
        ));
        source.object_uri = None;
        let mut event_times = retained.iter().map(event_time_unix_nano);
        let first_event_time = event_times
            .next()
            .transpose()?
            .expect("retained segment is non-empty");
        let (minimum, maximum) = event_times.try_fold(
            (first_event_time, first_event_time),
            |(minimum, maximum), value| {
                let value = value?;
                anyhow::Ok((minimum.min(value), maximum.max(value)))
            },
        )?;
        source.min_event_time_unix_nano = minimum;
        source.max_event_time_unix_nano = maximum;
        let (minimum_acknowledged_at, maximum_acknowledged_at) =
            acknowledgement_time_bounds(&retained)?;
        let dedupe_receipt =
            archive_dedupe_receipts(&mut transaction, &bucket, &rewrite_prefix, &retained)?;
        let replacement = ArchiveSegment {
            signal: segment.signal,
            source,
            object_uri: gcs_uri(&bucket, &key),
            parquet_bytes: parquet.len() as u64,
            parquet_sha256,
            min_acknowledged_at_unix_nano: minimum_acknowledged_at,
            max_acknowledged_at_unix_nano: maximum_acknowledged_at,
            dedupe_receipt,
        };
        apply_catalog_upsert(
            &current_catalog,
            &mut catalog_root,
            storage_segment::CatalogEntry {
                key: segment_catalog_key(&replacement),
                value: serde_json::to_vec(&replacement)?,
            },
            &bucket,
            &mut gc_candidates,
            &mut live_objects,
        )?;
        live_objects.insert_uri(&replacement.object_uri)?;
        live_objects.insert_uri(&replacement.dedupe_receipt.object_uri)?;
        segment_count = segment_count.saturating_add(1);
        include_retention_oldest(
            &mut scan.oldest_retained_event_time_unix_nano,
            replacement.source.min_event_time_unix_nano,
        );
    }

    for (hash, decrement) in blob_decrements {
        let key = format!("blob/{hash}");
        let entry = current_catalog
            .lookup(&catalog_root, &key)?
            .context("expired event references a blob absent from the archive catalog")?;
        let ArchiveCatalogItem::Blob(mut blob) = decode_archive_catalog_entry(entry)? else {
            bail!("archive blob key resolved to a segment");
        };
        if decrement > blob.reference_count {
            bail!("archive blob reference count underflow");
        }
        apply_catalog_remove(
            &current_catalog,
            &mut catalog_root,
            &key,
            &bucket,
            &mut gc_candidates,
            &mut live_objects,
        )?;
        blob.reference_count -= decrement;
        if blob.reference_count == 0 {
            blob_count = blob_count
                .checked_sub(1)
                .context("retained blob count underflow")?;
            gc_candidates.insert_uri(&blob.object_uri)?;
        } else {
            apply_catalog_upsert(
                &current_catalog,
                &mut catalog_root,
                storage_segment::CatalogEntry {
                    key: blob_catalog_key(&blob),
                    value: serde_json::to_vec(&blob)?,
                },
                &bucket,
                &mut gc_candidates,
                &mut live_objects,
            )?;
            live_objects.insert_uri(&blob.object_uri)?;
        }
    }

    let mut receipt_reader = current_catalog.reader_after(&catalog_root, "receipt/")?;
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
            &current_catalog,
            &mut catalog_root,
            &entry.key,
            &bucket,
            &mut gc_candidates,
            &mut live_objects,
        )?;
        gc_candidates.insert_uri(&receipt.object_uri)?;
        dedupe_receipt_count = dedupe_receipt_count
            .checked_sub(1)
            .context("archive dedupe receipt count underflow")?;
        removed_receipts += 1;
    }

    if catalog_root.entry_count
        != segment_count
            .saturating_add(blob_count)
            .saturating_add(dedupe_receipt_count)
    {
        bail!("bounded retention catalog count disagrees with its manifest totals");
    }
    let retained_watermarks = retained_watermarks_from_catalog(&current_catalog, &catalog_root)?;
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
        .and(current.oldest_event_time_unix_nano)
        .or(scan.oldest_retained_event_time_unix_nano);

    let gc_prefix = format!("{rewrite_prefix}/gc-plan");
    let gc_plan = build_gc_catalog_from_spills(
        object_store,
        &gc_prefix,
        &gc_candidates,
        &live_objects,
        &spill_parent,
    )?;
    let gc_object_count = gc_plan
        .as_ref()
        .map(|plan| plan.root.entry_count)
        .unwrap_or_default();
    let manifest = ArchiveManifest {
        format_version: ARCHIVE_FORMAT_VERSION,
        generated_at: now.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
        source_cluster_id: current.source_cluster_id.clone(),
        source_node_id: current.source_node_id.clone(),
        raft_snapshot_index: current.raft_snapshot_index,
        event_count,
        event_id_digest_algorithm: "xor-sha256-v1".to_string(),
        event_id_sha256: hex::encode(event_id_digest),
        event_content_digest_algorithm: "xor-sha256-v1".to_string(),
        event_content_sha256: hex::encode(event_content_digest),
        retention_generation: target_generation,
        watermarks: current.watermarks,
        retained_watermarks,
        oldest_event_time_unix_nano,
        epochs: current.epochs.clone(),
        catalog_uri: current.catalog_uri.clone(),
        catalog_root,
        segment_count,
        blob_count,
        dedupe_receipt_count,
        gc_plan_uri: gc_plan.as_ref().map(|_| gcs_uri(&bucket, &gc_prefix)),
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
        retention_scan,
        segments: Vec::new(),
        blobs: Vec::new(),
        gc_object_uris: Vec::new(),
    };
    validate_archive_manifest(&manifest)?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let commit = transaction.commit(storage_segment::ArchiveObject::new(
        manifest_key.clone(),
        manifest_bytes,
        "application/json",
    ))?;
    let receipt = ArchiveReceipt {
        manifest_uri: gcs_uri(&bucket, &manifest_key),
        manifest_sha256: commit.manifest.sha256,
        manifest,
    };
    finish_bounded_retention_commit(journal, &current, receipt).map(|mut outcome| {
        outcome.expired_events = expired_events;
        outcome.replaced_segments = replaced_segments;
        outcome.removed_segments = removed_segments;
        outcome
    })
}
