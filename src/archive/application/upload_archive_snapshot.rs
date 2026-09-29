//! Uploading a captured snapshot's segments, blobs and dedupe receipts, then
//! its catalog and manifest.

use std::sync::Arc;

use anyhow::{bail, Context, Result};

use crate::archive::application::archive_raw_storage_to_gcs::verify_local_archive_prefix;
use crate::archive::application::archive_receipts::ArchiveReceipt;
use crate::archive::domain::archive_catalog_item::{
    blob_catalog_key, segment_catalog_key, ArchiveCatalogItem,
};
use crate::archive::domain::archive_event_time::acknowledgement_time_bounds;
use crate::archive::domain::archive_manifest::{
    empty_dedupe_receipt, ArchiveBlob, ArchiveManifest, ArchiveSegment, ARCHIVE_FORMAT_VERSION,
};
use crate::archive::domain::archive_manifest_validator::validate_archive_manifest;
use crate::archive::domain::event_set_digest::{
    decode_event_content_digest, decode_event_id_digest, include_event_content, include_event_id,
};
use crate::archive::domain::portable_manifest::portable_manifest;
use crate::archive::infrastructure::archive_catalog::{
    catalog_for_uri, decode_archive_catalog_entry,
};
use crate::archive::infrastructure::archive_commit_state::read_commit_state;
use crate::archive::infrastructure::archive_gc_pending_store::reconcile_staged_archive_gc;
use crate::archive::infrastructure::archive_integrity::sha256;
use crate::archive::infrastructure::archive_upload_intent::{
    prepare_archive_upload_intent, recover_uploaded_archive,
};
use crate::archive::infrastructure::dedupe_receipt_archive::archive_dedupe_receipts;
use crate::archive::infrastructure::gc_plan_builder::{
    build_gc_catalog_from_spills, cleanup_observed_catalog_pages, record_catalog_page,
    spill_pending_archive_gc,
};
use crate::archive::infrastructure::gcs_uri::{gcs_destination, gcs_uri, split_gcs_uri};
use crate::archive::infrastructure::parquet_event_codec::{encode_parquet, SiftSignalPartitioner};
use crate::archive::infrastructure::spill_catalog::{
    add_spill_blob_reference, SpillBlobReference, SpillCatalog,
};
use crate::journal::infrastructure::storage::raw_storage::RawStorage;
use crate::storage::SegmentManifest;
use crate::SignalKind;

pub(super) fn archive_gcs_captured_streaming(
    storage: &RawStorage,
    destination_uri: &str,
    mut captured_cursor: u64,
    mut captured_segments: Vec<(SignalKind, SegmentManifest)>,
) -> Result<ArchiveReceipt> {
    let destination = service_backup::BackupDestination::from_uri(destination_uri)?;
    let (bucket, prefix) = gcs_destination(&destination)?;
    reconcile_staged_archive_gc(storage.root())?;
    let layout: crate::storage::LayoutManifest = serde_json::from_slice(
        &std::fs::read(storage.root().join("layout.json"))
            .context("read Sift layout for archive identity")?,
    )
    .context("decode Sift layout for archive identity")?;
    let previous_state = read_commit_state(storage.root())?;
    let previous = previous_state.as_ref().map(|state| &state.manifest);
    if previous.is_some_and(|manifest| manifest.source_cluster_id != layout.cluster_id) {
        bail!("committed archive belongs to a different Sift cluster");
    }
    if previous.is_some_and(|manifest| manifest.retention_scan.is_some()) {
        bail!("bounded retention must complete before a newer archive suffix is committed");
    }
    let canonical_destination = format!("gs://{bucket}/{prefix}");
    let intent = prepare_archive_upload_intent(
        storage.root(),
        &canonical_destination,
        &prefix,
        &layout.cluster_id,
        captured_cursor,
        previous_state.as_ref(),
    )?;
    let object_store: Arc<dyn storage_object::ObjectStore> =
        Arc::new(storage_object::GcsObjectStore::new(&bucket, "")?);
    if let Some(receipt) = recover_uploaded_archive(object_store.as_ref(), &intent)? {
        return Ok(receipt);
    }
    if captured_cursor < intent.captured_cursor {
        bail!("captured archive cursor moved behind its durable upload intent");
    }
    captured_cursor = intent.captured_cursor;
    if captured_segments.iter().any(|(_, segment)| {
        segment.first_cursor <= captured_cursor && segment.last_cursor > captured_cursor
    }) {
        bail!("a local segment crosses the durable archive upload cursor");
    }
    captured_segments.retain(|(_, segment)| segment.last_cursor <= captured_cursor);
    let coordinator = storage_segment::ArchiveCoordinator::new(object_store.clone());
    let mut archive_transaction = coordinator.begin();
    let archive_prefix = intent.archive_prefix.clone();
    let previous_catalog = previous
        .map(|manifest| catalog_for_uri(&manifest.catalog_uri))
        .transpose()?;
    let previously_covered = previous
        .map(|manifest| manifest.raft_snapshot_index)
        .unwrap_or_default();
    if captured_cursor < previously_covered {
        bail!(
            "captured archive cursor {captured_cursor} is behind committed prefix {previously_covered}"
        );
    }

    let spill_parent = storage.root().join("tmp");
    let mut catalog_updates = SpillCatalog::new(&spill_parent, "archive-updates-")?;
    let mut referenced = SpillCatalog::new(&spill_parent, "archive-blob-counts-")?;
    let mut gc_candidates = SpillCatalog::new(&spill_parent, "archive-obsolete-")?;
    let mut live_objects = SpillCatalog::new(&spill_parent, "archive-live-")?;
    spill_pending_archive_gc(storage.root(), &mut gc_candidates)?;
    if let Some(state) = &previous_state {
        gc_candidates.insert_uri(&state.manifest_uri)?;
    }
    if let Some(previous) = previous {
        if let (Some(gc_uri), Some(gc_root)) = (&previous.gc_plan_uri, &previous.gc_plan_root) {
            let (gc_bucket, _) = split_gcs_uri(gc_uri)?;
            let gc_catalog = catalog_for_uri(gc_uri)?;
            for key in gc_catalog.page_keys(gc_root)? {
                gc_candidates.insert_uri(&gcs_uri(&gc_bucket, &key?))?;
            }
        }
    }

    let mut event_count = previous.map(|value| value.event_count).unwrap_or_default();
    let mut event_id_digest = previous
        .map(|value| decode_event_id_digest(&value.event_id_sha256))
        .transpose()?
        .unwrap_or([0; 32]);
    let mut event_content_digest = previous
        .map(|value| decode_event_content_digest(&value.event_content_sha256))
        .transpose()?
        .unwrap_or([0; 32]);
    let mut watermarks = previous.map(|value| value.watermarks).unwrap_or_default();
    let mut retained_watermarks = previous
        .map(|value| value.retained_watermarks)
        .unwrap_or_default();
    let retention_generation = previous
        .map(|value| value.retention_generation)
        .unwrap_or_default();
    let mut oldest_event_time_unix_nano =
        previous.and_then(|value| value.oldest_event_time_unix_nano);
    let mut new_segment_count = 0_u64;
    verify_local_archive_prefix(storage, previously_covered, captured_cursor)?;
    captured_segments.sort_by_key(|(_, segment)| segment.first_cursor);

    for (signal, source) in captured_segments {
        let portable_source = portable_manifest(source.clone(), signal);
        let covered_through = previous
            .map(|manifest| manifest.watermarks.through(signal))
            .unwrap_or_default();
        if source.last_cursor <= covered_through {
            if let (Some(previous), Some(catalog)) = (previous, &previous_catalog) {
                let probe = ArchiveSegment {
                    signal,
                    source: portable_source.clone(),
                    object_uri: String::new(),
                    parquet_bytes: 0,
                    parquet_sha256: String::new(),
                    min_acknowledged_at_unix_nano: 0,
                    max_acknowledged_at_unix_nano: 0,
                    dedupe_receipt: empty_dedupe_receipt(),
                };
                if let Some(entry) =
                    catalog.lookup(&previous.catalog_root, &segment_catalog_key(&probe))?
                {
                    let ArchiveCatalogItem::Segment(committed) =
                        decode_archive_catalog_entry(entry)?
                    else {
                        bail!("archive segment catalog key resolved to a blob");
                    };
                    if committed.signal != signal || committed.source != portable_source {
                        bail!(
                            "immutable segment {} changed after archive commit",
                            source.segment_id
                        );
                    }
                }
            }
            continue;
        }
        if source.first_cursor <= covered_through {
            bail!(
                "local segment {} crosses committed archive cursor {}",
                source.segment_id,
                covered_through
            );
        }
        let events = storage.read_segment_events(signal, &source)?;
        let partition = events
            .first()
            .map(|event| storage_segment::Partitioner::partition(&SiftSignalPartitioner, event))
            .transpose()?
            .context("immutable segment must contain at least one event")?;
        if partition != signal.to_string() {
            bail!("segment partition disagrees with its signal");
        }
        for event in &events {
            event_count = event_count.saturating_add(1);
            include_event_id(&mut event_id_digest, &event.event.event_id);
            include_event_content(&mut event_content_digest, event)?;
            watermarks.include(signal, event.cursor);
            retained_watermarks.include(signal, event.cursor);
            for reference in &event.event.blob_refs {
                add_spill_blob_reference(&mut referenced, reference)?;
            }
        }
        let parquet = encode_parquet(&events)?;
        let parquet_sha256 = sha256(&parquet);
        let key = format!(
            "{archive_prefix}/segments/{partition}/{}.parquet",
            source.segment_id
        );
        archive_transaction.put(storage_segment::ArchiveObject::new(
            key.clone(),
            parquet.clone(),
            "application/vnd.apache.parquet",
        ))?;
        let (minimum_acknowledged_at, maximum_acknowledged_at) =
            acknowledgement_time_bounds(&events)?;
        let dedupe_receipt =
            archive_dedupe_receipts(&mut archive_transaction, &bucket, &archive_prefix, &events)?;
        let segment = ArchiveSegment {
            signal,
            source: portable_source,
            object_uri: gcs_uri(&bucket, &key),
            parquet_bytes: parquet.len() as u64,
            parquet_sha256,
            min_acknowledged_at_unix_nano: minimum_acknowledged_at,
            max_acknowledged_at_unix_nano: maximum_acknowledged_at,
            dedupe_receipt,
        };
        oldest_event_time_unix_nano = Some(
            oldest_event_time_unix_nano
                .map(|oldest| oldest.min(segment.source.min_event_time_unix_nano))
                .unwrap_or(segment.source.min_event_time_unix_nano),
        );
        catalog_updates.upsert(segment_catalog_key(&segment), serde_json::to_vec(&segment)?)?;
        live_objects.insert_uri(&segment.object_uri)?;
        live_objects.insert_uri(&segment.dedupe_receipt.object_uri)?;
        new_segment_count = new_segment_count.saturating_add(1);
    }

    let mut new_blob_count = 0_u64;
    for entry in referenced.reader()? {
        let counted: SpillBlobReference =
            serde_json::from_slice(&entry?.value).context("decode archive spill blob reference")?;
        let catalog_key = format!("blob/{}", counted.reference.hash);
        let existing = match (previous, &previous_catalog) {
            (Some(previous), Some(catalog)) => catalog
                .lookup(&previous.catalog_root, &catalog_key)?
                .map(decode_archive_catalog_entry)
                .transpose()?,
            _ => None,
        };
        let blob = if let Some(ArchiveCatalogItem::Blob(mut blob)) = existing {
            if blob.reference != counted.reference || blob.reference_count == 0 {
                bail!(
                    "immutable blob {} changed after archive commit",
                    counted.reference.hash
                );
            }
            blob.reference_count = blob.reference_count.saturating_add(counted.count);
            blob
        } else if existing.is_some() {
            bail!("archive blob catalog key resolved to a segment");
        } else {
            let bytes = storage.read_blob(&counted.reference.hash)?;
            if bytes.len() as u64 != counted.reference.size {
                bail!(
                    "blob {} size changed before archive",
                    counted.reference.hash
                );
            }
            let digest = counted.reference.hash.trim_start_matches("sha256:");
            let key = format!("{archive_prefix}/blobs/{digest}.blob");
            archive_transaction.put(storage_segment::ArchiveObject::new(
                key.clone(),
                bytes,
                "application/octet-stream",
            ))?;
            new_blob_count = new_blob_count.saturating_add(1);
            ArchiveBlob {
                reference: counted.reference,
                object_uri: gcs_uri(&bucket, &key),
                reference_count: counted.count,
            }
        };
        catalog_updates.upsert(blob_catalog_key(&blob), serde_json::to_vec(&blob)?)?;
        live_objects.insert_uri(&blob.object_uri)?;
    }

    let catalog_prefix = format!("{archive_prefix}/catalog");
    let catalog_runtime =
        storage_segment::PagedCatalog::new(object_store.clone(), &catalog_prefix)?;
    let catalog_root = if let Some(previous) = previous {
        let (previous_bucket, _) = split_gcs_uri(&previous.catalog_uri)?;
        if previous_bucket != bucket {
            bail!("committed archive catalog moved to a different bucket");
        }
        let mut root = previous.catalog_root.clone();
        for entry in catalog_updates.reader()? {
            let mutation = catalog_runtime.upsert(&root, entry?)?;
            for key in &mutation.obsolete_page_keys {
                gc_candidates.insert_uri(&gcs_uri(&bucket, key))?;
                live_objects.remove_uri(&gcs_uri(&bucket, key))?;
            }
            for key in &mutation.written_page_keys {
                live_objects.insert_uri(&gcs_uri(&bucket, key))?;
            }
            root = mutation.root;
        }
        root
    } else {
        let mut written = SpillCatalog::new(&spill_parent, "archive-catalog-written-")?;
        match catalog_runtime.build_sorted_observed(catalog_updates.reader()?, |reference| {
            record_catalog_page(&mut written, reference)
        }) {
            Ok(build) => build.root,
            Err(error) => {
                cleanup_observed_catalog_pages(object_store.as_ref(), &written)?;
                return Err(error.into());
            }
        }
    };
    let segment_count = previous
        .map(|manifest| manifest.segment_count)
        .unwrap_or_default()
        .saturating_add(new_segment_count);
    let blob_count = previous
        .map(|manifest| manifest.blob_count)
        .unwrap_or_default()
        .saturating_add(new_blob_count);
    let dedupe_receipt_count = previous
        .map(|manifest| manifest.dedupe_receipt_count)
        .unwrap_or_default();
    if catalog_root.entry_count
        != segment_count
            .saturating_add(blob_count)
            .saturating_add(dedupe_receipt_count)
    {
        bail!("archive catalog root count does not match segment and blob counts");
    }
    let gc_prefix = format!("{archive_prefix}/gc-plan");
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
        generated_at: intent.generated_at,
        source_cluster_id: layout.cluster_id,
        source_node_id: layout.node_id,
        raft_snapshot_index: captured_cursor,
        event_count,
        event_id_digest_algorithm: "xor-sha256-v1".to_string(),
        event_id_sha256: hex::encode(event_id_digest),
        event_content_digest_algorithm: "xor-sha256-v1".to_string(),
        event_content_sha256: hex::encode(event_content_digest),
        retention_generation,
        watermarks,
        retained_watermarks,
        oldest_event_time_unix_nano,
        epochs: storage.epoch_maps(),
        catalog_uri: gcs_uri(&bucket, &catalog_prefix),
        catalog_root,
        segment_count,
        blob_count,
        dedupe_receipt_count,
        gc_plan_uri: gc_plan.as_ref().map(|_| gcs_uri(&bucket, &gc_prefix)),
        gc_plan_root: gc_plan.map(|plan| plan.root),
        gc_object_count,
        retention_delta: None,
        retention_scan: previous.and_then(|manifest| manifest.retention_scan.clone()),
        segments: Vec::new(),
        blobs: Vec::new(),
        gc_object_uris: Vec::new(),
    };
    validate_archive_manifest(&manifest)?;
    let manifest_key = format!("{archive_prefix}/manifest.json");
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let commit = archive_transaction.commit(storage_segment::ArchiveObject::new(
        manifest_key.clone(),
        manifest_bytes,
        "application/json",
    ))?;
    Ok(ArchiveReceipt {
        manifest_uri: gcs_uri(&bucket, &manifest_key),
        manifest_sha256: commit.manifest.sha256,
        manifest,
    })
}
