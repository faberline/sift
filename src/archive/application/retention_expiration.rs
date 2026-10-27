//! Expiring committed events past the retention window in bounded passes.

use crate::archive::application::archive_services::ArchiveServices;

use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::archive::application::gc_plan_builder::build_gc_catalog_from_spills;
use crate::archive::application::manifest_validation::validate_archive_manifest;
use crate::archive::application::reconcile_committed_retention::reconcile_live_committed_retention;
use crate::archive::application::retention_catalog_ops::{
    apply_catalog_remove, apply_catalog_upsert,
};
use crate::archive::application::retention_due::committed_has_expired_dedupe_receipt;
use crate::archive::application::retention_manifest_commit::{
    finish_bounded_retention_commit, validate_retention_receipt_source,
};
use crate::archive::application::retention_pass::RetentionPass;
use crate::archive::application::rewrite_retained_segment::rewrite_retained_segment;
use crate::archive::domain::archive_catalog_item::{
    dedupe_receipt_catalog_key, segment_catalog_key, ArchiveCatalogItem,
};
use crate::archive::domain::catalog_decode::decode_archive_catalog_entry;
use crate::archive::domain::control_values::ArchiveCommitState;
use crate::archive::domain::gcs_uri::{gcs_uri, split_gcs_uri};
use crate::archive::domain::retention_policy::{
    include_retention_oldest, RETENTION_SCAN_BATCH_SEGMENTS,
};
use crate::shared_kernel::archive_manifest::{ArchiveManifest, ArchiveRetentionScan};
use crate::shared_kernel::archive_receipts::{ArchiveReceipt, ExpirationReceipt};
use crate::shared_kernel::sha256_hex::sha256_hex;

/// Remove events older than the fixed 180-day boundary from the committed
/// manifest. A mixed Parquet segment is rewritten with only retained rows.
/// The new objects and manifest commit before the prior objects are deleted.
pub(crate) fn expire_committed_events_at(
    services: &ArchiveServices,
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    now: DateTime<Utc>,
) -> Result<ExpirationReceipt> {
    expire_committed_events_bounded_at(services, journal, now)
}

fn expire_committed_events_bounded_at(
    services: &ArchiveServices,
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    now: DateTime<Utc>,
) -> Result<ExpirationReceipt> {
    const BATCH_PARQUET_BYTES: u64 = 64 * 1024 * 1024;

    let root = journal.storage().root();
    let (current_state, current) = ensure_retention_head_reconciled(services, journal, root)?;
    let (requested_cutoff_nanos, receipt_cutoff_nanos) = retention_cutoffs(now)?;
    if current.retention_scan.is_none()
        && current
            .oldest_event_time_unix_nano
            .is_none_or(|oldest| oldest >= requested_cutoff_nanos)
        && !committed_has_expired_dedupe_receipt(services, &current, receipt_cutoff_nanos)?
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
    let object_store = services.resources.gcs_store(&bucket)?;
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
    if let Some(receipt) =
        resume_uploaded_retention_manifest(&object_store, &bucket, &manifest_key, &current_state)?
    {
        return finish_bounded_retention_commit(services, journal, &current, receipt);
    }

    let current_catalog = services.resources.catalog_for_uri(&current.catalog_uri)?;
    let mut reader =
        current_catalog.reader_after(&current.catalog_root, &scan.after_catalog_key)?;
    let spill_parent = root.join("tmp");
    let mut pass = RetentionPass::new(services, root, &current_state, &current, &spill_parent)?;

    let mut transaction = object_store.transaction();
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
        let bytes = services.cached_segment_bytes(root, &segment)?;
        let events = services.decode_parquet(&bytes)?;
        services.verify_archive_segment(&segment, &events)?;
        let (retained, segment_expired, has_active_expired_receipt) =
            pass.partition_expired_events(events, cutoff, now)?;
        if segment_expired == 0 {
            include_retention_oldest(
                &mut scan.oldest_retained_event_time_unix_nano,
                segment.source.min_event_time_unix_nano,
            );
            continue;
        }
        pass.expired_events = pass.expired_events.saturating_add(segment_expired);
        apply_catalog_remove(
            &current_catalog,
            &mut pass.catalog_root,
            &entry_key,
            &bucket,
            &mut pass.gc_candidates,
            &mut pass.live_objects,
        )?;
        pass.gc_candidates.insert_uri(&segment.object_uri)?;
        pass.gc_candidates
            .insert_uri(&segment.dedupe_receipt.object_uri)?;
        pass.segment_count = pass
            .segment_count
            .checked_sub(1)
            .context("retained segment count underflow")?;
        if has_active_expired_receipt {
            services
                .fetch_dedupe_receipts(&segment.dedupe_receipt)
                .context("verify active dedupe receipt before retaining it")?;
            apply_catalog_upsert(
                &current_catalog,
                &mut pass.catalog_root,
                crate::archive::domain::resource_ports::CatalogEntry {
                    key: dedupe_receipt_catalog_key(&segment.dedupe_receipt),
                    value: serde_json::to_vec(&segment.dedupe_receipt)?,
                },
                &bucket,
                &mut pass.gc_candidates,
                &mut pass.live_objects,
            )?;
            pass.live_objects
                .insert_uri(&segment.dedupe_receipt.object_uri)?;
            pass.dedupe_receipt_count = pass.dedupe_receipt_count.saturating_add(1);
        }
        if retained.is_empty() {
            pass.removed_segments += 1;
            continue;
        }

        pass.replaced_segments += 1;
        let replacement = rewrite_retained_segment(
            services,
            &mut transaction,
            &segment,
            &retained,
            &bucket,
            &rewrite_prefix,
        )?;
        apply_catalog_upsert(
            &current_catalog,
            &mut pass.catalog_root,
            crate::archive::domain::resource_ports::CatalogEntry {
                key: segment_catalog_key(&replacement),
                value: serde_json::to_vec(&replacement)?,
            },
            &bucket,
            &mut pass.gc_candidates,
            &mut pass.live_objects,
        )?;
        pass.live_objects.insert_uri(&replacement.object_uri)?;
        pass.live_objects
            .insert_uri(&replacement.dedupe_receipt.object_uri)?;
        pass.segment_count = pass.segment_count.saturating_add(1);
        include_retention_oldest(
            &mut scan.oldest_retained_event_time_unix_nano,
            replacement.source.min_event_time_unix_nano,
        );
    }

    pass.apply_blob_decrements(&current_catalog, &bucket)?;
    pass.expire_dedupe_receipts(&current_catalog, &bucket, receipt_cutoff_nanos)?;
    let finalized = pass.finalize_retention_scan(
        &current_catalog,
        &scan,
        scan_complete,
        current.oldest_event_time_unix_nano,
    )?;

    let gc_prefix = format!("{rewrite_prefix}/gc-plan");
    let gc_plan = build_gc_catalog_from_spills(
        services,
        object_store,
        &gc_prefix,
        &pass.gc_candidates,
        &pass.live_objects,
        &spill_parent,
    )?;
    let gc_plan_uri = gc_plan.as_ref().map(|_| gcs_uri(&bucket, &gc_prefix));
    let manifest = pass.build_retention_manifest(
        now,
        &current,
        &current_state,
        target_generation,
        &scan,
        finalized,
        gc_plan,
        gc_plan_uri,
    );
    validate_archive_manifest(&manifest)?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let commit = transaction.commit(crate::archive::domain::resource_ports::ArchiveObject::new(
        manifest_key.clone(),
        manifest_bytes,
        "application/json",
    ))?;
    let receipt = ArchiveReceipt {
        manifest_uri: gcs_uri(&bucket, &manifest_key),
        manifest_sha256: commit.manifest.sha256,
        manifest,
    };
    finish_bounded_retention_commit(services, journal, &current, receipt).map(|mut outcome| {
        outcome.expired_events = pass.expired_events;
        outcome.replaced_segments = pass.replaced_segments;
        outcome.removed_segments = pass.removed_segments;
        outcome
    })
}

/// Reconcile the journal's retention with the archive, and read the committed
/// commit state and manifest. The journal must hold every event up to the
/// archive's snapshot, the archive's retained events plus its own suffix, and
/// the archive's retention generation.
fn ensure_retention_head_reconciled(
    services: &ArchiveServices,
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    root: &Path,
) -> Result<(ArchiveCommitState, ArchiveManifest)> {
    reconcile_live_committed_retention(services, journal)?;
    let current_state = services
        .read_commit_state(root)?
        .context("180-day expiration requires a committed remote manifest")?;
    let current = services
        .fetch_verified_committed_root(root)?
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
    Ok((current_state, current))
}

/// The event cutoff, 180 days before `now`, and the dedupe receipt cutoff,
/// one idempotency window before it, in Unix nanoseconds.
fn retention_cutoffs(now: DateTime<Utc>) -> Result<(i64, i64)> {
    let requested_cutoff_nanos = (now - chrono::Duration::days(180))
        .timestamp_nanos_opt()
        .context("180-day retention cutoff is outside the nanosecond range")?;
    let receipt_cutoff_nanos = (now
        - chrono::Duration::seconds(
            crate::shared_kernel::idempotency_window::IDEMPOTENCY_WINDOW_SECONDS,
        ))
    .timestamp_nanos_opt()
    .context("dedupe receipt cleanup cutoff is outside the nanosecond range")?;
    Ok((requested_cutoff_nanos, receipt_cutoff_nanos))
}

/// The manifest an earlier attempt of this pass already uploaded, if there is
/// one. A crash after the upload resumes from it instead of rewriting.
fn resume_uploaded_retention_manifest(
    object_store: &crate::archive::application::object_service::ObjectService,
    bucket: &str,
    manifest_key: &str,
    current_state: &ArchiveCommitState,
) -> Result<Option<ArchiveReceipt>> {
    match object_store.get(manifest_key) {
        Ok(object) => {
            let manifest: ArchiveManifest = serde_json::from_slice(&object.bytes)
                .context("decode an uploaded bounded retention manifest")?;
            validate_archive_manifest(&manifest)?;
            let receipt = ArchiveReceipt {
                manifest_uri: gcs_uri(bucket, manifest_key),
                manifest_sha256: sha256_hex(&object.bytes),
                manifest,
            };
            validate_retention_receipt_source(&receipt, current_state)?;
            Ok(Some(receipt))
        }
        Err(storage_object::ObjectStoreError::NotFound { .. }) => Ok(None),
        Err(error) => Err(error.into()),
    }
}
