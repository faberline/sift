//! Restoring an archive into an empty data directory, page by page.

use crate::archive::application::archive_services::ArchiveServices;

use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::Utc;

use crate::archive::application::manifest_validation::validate_archive_manifest;
use crate::archive::application::signal_service::SignalService as ArchiveSignalStream;
use crate::archive::domain::archive_catalog_item::ArchiveCatalogItem;
use crate::archive::domain::archive_event_time::event_time_unix_nano;
use crate::archive::domain::blob_hash_set::BlobHashSet;
use crate::archive::domain::catalog_decode::decode_archive_catalog_entry;
use crate::archive::domain::event_set_digest::{include_event_content, include_event_id};
use crate::node::domain::storage_role::StorageRole;
use crate::node::infrastructure::data_layout::DataLayout;
use crate::shared_kernel::archive_manifest::ArchiveManifest;
use crate::shared_kernel::archive_receipts::ArchiveReceipt;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::shared_kernel::sha256_hex::sha256_hex;
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;

pub(super) fn restore_gcs_into_empty(
    services: &ArchiveServices,
    manifest_uri: &str,
    target: &Path,
) -> Result<ArchiveManifest> {
    services.require_empty_restore_target(target)?;
    let manifest_bytes = services.resources.fetch_object(manifest_uri)?;
    let manifest_sha256 = sha256_hex(&manifest_bytes);
    let mut manifest: ArchiveManifest =
        serde_json::from_slice(&manifest_bytes).context("decode Sift archive manifest")?;
    validate_archive_manifest(&manifest)?;

    let layout = DataLayout::open(target, StorageRole::All)?;
    if layout.manifest().cluster_id == manifest.source_cluster_id {
        bail!("cold restore must create a new Sift cluster ID");
    }
    drop(layout);
    services
        .restore_target
        .write_epochs(target, &manifest.epochs)?;
    let journal = services.restore_target.open_journal(target)?;
    let hot_cutoff_nanos = (Utc::now() - chrono::Duration::days(30))
        .timestamp_nanos_opt()
        .context("30-day hot restore cutoff is outside the nanosecond range")?;
    let catalog = services.resources.catalog_for_uri(&manifest.catalog_uri)?;
    let stream = |signal| -> Result<ArchiveSignalStream> {
        let prefix = format!("segment/{signal}/");
        Ok(services.resources.signal_stream(
            target,
            catalog.reader_after(&manifest.catalog_root, &prefix)?,
            signal,
            0,
        ))
    };
    let mut streams = [
        stream(SignalKind::Log)?,
        stream(SignalKind::Metric)?,
        stream(SignalKind::Span)?,
    ];
    let mut page = Vec::with_capacity(10_000);
    let spill_parent = target.join("tmp");
    let mut hot_blob_hashes = services
        .resources
        .spill(&spill_parent, "restore-hot-blobs-")?;
    let mut blob_reference_counts = services
        .resources
        .spill(&spill_parent, "restore-blob-counts-")?;
    let mut event_count = 0_u64;
    let mut last_cursor = 0_u64;
    let mut event_id_digest = [0_u8; 32];
    let mut event_content_digest = [0_u8; 32];
    let mut restored_watermarks = ArchiveWatermarks::default();
    let mut oldest_event_time_unix_nano = None::<i64>;
    loop {
        let mut next = None::<(usize, u64)>;
        for (index, stream) in streams.iter_mut().enumerate() {
            if let Some(cursor) = stream.peek_cursor()? {
                if next.is_none_or(|(_, current)| cursor < current) {
                    next = Some((index, cursor));
                }
            }
        }
        let Some((index, cursor)) = next else {
            break;
        };
        if cursor <= last_cursor {
            bail!("archive cursor {cursor} is not globally strictly increasing");
        }
        let event = streams[index]
            .pop_event()?
            .context("archive stream head disappeared during restore")?;
        event.event.validate()?;
        last_cursor = cursor;
        event_count = event_count.saturating_add(1);
        include_event_id(&mut event_id_digest, &event.event.event_id);
        include_event_content(&mut event_content_digest, &event)?;
        let event_time = event_time_unix_nano(&event)?;
        oldest_event_time_unix_nano = Some(
            oldest_event_time_unix_nano
                .map(|oldest| oldest.min(event_time))
                .unwrap_or(event_time),
        );
        restored_watermarks.include(event.event.signal, event.cursor);
        for reference in &event.event.blob_refs {
            blob_reference_counts.add_u64(format!("blob/{}", reference.hash), 1)?;
        }
        page.push(event);
        if page.len() == 10_000 {
            restore_archive_page(
                &journal,
                std::mem::take(&mut page),
                hot_cutoff_nanos,
                &mut hot_blob_hashes,
            )?;
            page = Vec::with_capacity(10_000);
        }
    }
    if !page.is_empty() {
        restore_archive_page(&journal, page, hot_cutoff_nanos, &mut hot_blob_hashes)?;
    }
    let oldest_matches = match manifest.retention_scan {
        Some(_) => match (
            manifest.oldest_event_time_unix_nano,
            oldest_event_time_unix_nano,
        ) {
            (Some(lower_bound), Some(observed)) => lower_bound <= observed,
            (None, None) => true,
            _ => false,
        },
        None => oldest_event_time_unix_nano == manifest.oldest_event_time_unix_nano,
    };
    if event_count != manifest.event_count
        || hex::encode(event_id_digest) != manifest.event_id_sha256
        || hex::encode(event_content_digest) != manifest.event_content_sha256
        || !oldest_matches
        || last_cursor != manifest.retained_watermarks.max_cursor()
        || restored_watermarks != manifest.retained_watermarks
        || last_cursor > manifest.raft_snapshot_index
    {
        bail!("Sift archive manifest count, digest, watermark, or Raft snapshot index mismatch");
    }

    let blob_store = services.restore_target.open_blob_writer(target, 65_536)?;
    let mut restored_blobs = 0_u64;
    for entry in catalog.reader(&manifest.catalog_root)? {
        let ArchiveCatalogItem::Blob(blob) = decode_archive_catalog_entry(entry?)? else {
            continue;
        };
        restored_blobs = restored_blobs.saturating_add(1);
        let actual_references =
            blob_reference_counts.get_u64(&format!("blob/{}", blob.reference.hash))?;
        if actual_references == 0
            || (blob.reference_count > 0 && blob.reference_count != actual_references)
        {
            bail!(
                "archive blob {} reference count disagrees with restored events",
                blob.reference.hash
            );
        }
        let bytes = services.resources.fetch_object(&blob.object_uri)?;
        let actual_hash = format!("sha256:{}", sha256_hex(&bytes));
        if actual_hash != blob.reference.hash || bytes.len() as u64 != blob.reference.size {
            bail!(
                "restored blob {} failed hash/size verification",
                blob.reference.hash
            );
        }
        if hot_blob_hashes.contains_hash(&blob.reference.hash)? {
            let restored = blob_store.put(&bytes, blob.reference.encoding.clone())?;
            if restored != blob.reference {
                bail!(
                    "restored hot blob {} changed its content reference",
                    blob.reference.hash
                );
            }
        }
    }
    if blob_reference_counts.len() != restored_blobs || restored_blobs != manifest.blob_count {
        bail!("restored events reference blobs missing from the archive manifest");
    }

    let mut restored_receipt_segments = 0_u64;
    for entry in catalog.reader(&manifest.catalog_root)? {
        let ArchiveCatalogItem::Receipt(receipt) = decode_archive_catalog_entry(entry?)? else {
            continue;
        };
        let rows = services.fetch_dedupe_receipts(&receipt)?;
        journal.restore_archive_receipts(&rows, manifest.raft_snapshot_index)?;
        restored_receipt_segments = restored_receipt_segments.saturating_add(1);
    }
    if restored_receipt_segments != manifest.dedupe_receipt_count {
        bail!("archive dedupe receipt count disagrees with its manifest");
    }

    journal.set_restored_archive_head(&manifest)?;
    let receipt = ArchiveReceipt {
        manifest_uri: manifest_uri.to_string(),
        manifest_sha256,
        manifest: manifest.clone(),
    };
    services.adopt_verified_archive_receipt(target, &receipt)?;
    drop(journal);
    let mut layout = DataLayout::open(target, StorageRole::All)?;
    layout.mark_restored_from(manifest_uri)?;
    manifest.segments.clear();
    manifest.blobs.clear();
    manifest.gc_object_uris.clear();
    Ok(manifest)
}

fn restore_archive_page<H>(
    journal: &crate::journal::application::journal_service::JournalService<'_>,
    events: Vec<StoredEvent>,
    hot_cutoff_nanos: i64,
    hot_blob_hashes: &mut H,
) -> Result<()>
where
    H: BlobHashSet,
    anyhow::Error: From<H::Error>,
{
    let mut hot = Vec::with_capacity(events.len());
    let mut cold = Vec::new();
    for event in events {
        if event_time_unix_nano(&event)? >= hot_cutoff_nanos {
            for reference in &event.event.blob_refs {
                hot_blob_hashes.insert_hash(&reference.hash)?;
            }
            hot.push(event);
        } else {
            cold.push(event);
        }
    }
    journal.restore_stored_page(hot)?;
    journal.restore_archive_dedupe_page(&cold)
}
