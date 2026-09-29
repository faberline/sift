//! Uploading and fetching the dedupe receipts an archive carries.

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression as GzipCompression;
use std::io::{Read, Write};

use crate::archive::domain::archive_event_time::acknowledgement_time_unix_nano;
use crate::archive::domain::archive_manifest::ArchiveDedupeReceipt;
use crate::archive::domain::archive_manifest_validator::validate_dedupe_receipt;
use crate::archive::infrastructure::archive_integrity::{sha256, verify_bytes};
use crate::archive::infrastructure::gcs_uri::gcs_uri;
use crate::journal::domain::dedupe_receipt::DedupeReceipt;
use crate::shared_kernel::stored_event::StoredEvent;

pub(in crate::archive) fn archive_dedupe_receipts(
    transaction: &mut storage_segment::ArchiveTransaction,
    bucket: &str,
    archive_prefix: &str,
    events: &[StoredEvent],
) -> Result<ArchiveDedupeReceipt> {
    let mut receipts = Vec::with_capacity(events.len());
    for event in events {
        receipts.push(DedupeReceipt {
            project: event.event.project.clone(),
            event_id: event.event.event_id.clone(),
            cursor: event.cursor,
            acknowledged_at_unix_nano: acknowledgement_time_unix_nano(event)?,
        });
    }
    if receipts.is_empty() {
        bail!("archive dedupe receipt cannot be empty");
    }
    let mut encoder = GzEncoder::new(Vec::new(), GzipCompression::fast());
    encoder
        .write_all(&serde_json::to_vec(&receipts)?)
        .context("encode archive dedupe receipt payload")?;
    let bytes = encoder
        .finish()
        .context("finish archive dedupe receipt gzip")?;
    let digest = sha256(&bytes);
    let key = format!("{archive_prefix}/receipts/{digest}.json.gz");
    transaction.put(storage_segment::ArchiveObject::new(
        key.clone(),
        bytes.clone(),
        "application/gzip",
    ))?;
    Ok(ArchiveDedupeReceipt {
        object_uri: gcs_uri(bucket, &key),
        bytes: bytes.len() as u64,
        sha256: digest,
        entry_count: receipts.len() as u64,
        first_cursor: receipts.first().expect("non-empty receipt").cursor,
        last_cursor: receipts.last().expect("non-empty receipt").cursor,
        min_acknowledged_at_unix_nano: receipts
            .iter()
            .map(|receipt| receipt.acknowledged_at_unix_nano)
            .min()
            .expect("non-empty receipt"),
        max_acknowledged_at_unix_nano: receipts
            .iter()
            .map(|receipt| receipt.acknowledged_at_unix_nano)
            .max()
            .expect("non-empty receipt"),
    })
}

pub(in crate::archive) fn fetch_dedupe_receipts(
    receipt: &ArchiveDedupeReceipt,
) -> Result<Vec<DedupeReceipt>> {
    validate_dedupe_receipt(receipt)?;
    let bytes = service_backup::fetch_backup_object(&receipt.object_uri)?;
    verify_bytes(&receipt.sha256, receipt.bytes, &bytes, "dedupe receipt")?;
    let maximum = receipt.entry_count.saturating_mul(1024).saturating_add(1);
    let mut decoded = Vec::new();
    GzDecoder::new(bytes.as_slice())
        .take(maximum)
        .read_to_end(&mut decoded)
        .context("decompress archive dedupe receipt")?;
    if decoded.len() as u64 >= maximum {
        bail!("archive dedupe receipt exceeds its decoded size limit");
    }
    let rows: Vec<DedupeReceipt> =
        serde_json::from_slice(&decoded).context("decode archive dedupe receipt")?;
    if rows.len() as u64 != receipt.entry_count
        || rows.first().map(|row| row.cursor) != Some(receipt.first_cursor)
        || rows.last().map(|row| row.cursor) != Some(receipt.last_cursor)
        || rows.iter().map(|row| row.acknowledged_at_unix_nano).min()
            != Some(receipt.min_acknowledged_at_unix_nano)
        || rows.iter().map(|row| row.acknowledged_at_unix_nano).max()
            != Some(receipt.max_acknowledged_at_unix_nano)
        || rows
            .windows(2)
            .any(|window| window[0].cursor >= window[1].cursor)
        || rows
            .iter()
            .any(|row| row.project.is_empty() || row.event_id.is_empty() || row.cursor == 0)
    {
        bail!("archive dedupe receipt metadata does not match its rows");
    }
    Ok(rows)
}
