//! Whether the committed archive has events or dedupe receipts past retention.

use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::archive::domain::archive_catalog_item::ArchiveCatalogItem;
use crate::archive::domain::archive_manifest::ArchiveManifest;
use crate::archive::domain::archive_manifest_validator::validate_dedupe_receipt;
use crate::archive::infrastructure::archive_catalog::{
    catalog_for_uri, decode_archive_catalog_entry,
};
use crate::archive::infrastructure::verified_manifest_fetcher::fetch_verified_committed_root;

pub(super) fn committed_has_expired_dedupe_receipt(
    manifest: &ArchiveManifest,
    cutoff_unix_nano: i64,
) -> Result<bool> {
    if manifest.dedupe_receipt_count == 0 {
        return Ok(false);
    }
    let catalog = catalog_for_uri(&manifest.catalog_uri)?;
    let Some(entry) = catalog
        .reader_after(&manifest.catalog_root, "receipt/")?
        .next()
        .transpose()?
    else {
        bail!("archive manifest counts dedupe receipts but its catalog has none");
    };
    if !entry.key.starts_with("receipt/") {
        bail!("archive receipt count disagrees with its catalog key range");
    }
    let ArchiveCatalogItem::Receipt(receipt) = decode_archive_catalog_entry(entry)? else {
        bail!("archive receipt key resolved to another catalog item");
    };
    validate_dedupe_receipt(&receipt)?;
    Ok(receipt.max_acknowledged_at_unix_nano < cutoff_unix_nano)
}

#[doc(hidden)]
pub fn retention_due_at(root: &Path, now: DateTime<Utc>) -> Result<bool> {
    let Some(manifest) = fetch_verified_committed_root(root)? else {
        return Ok(false);
    };
    if manifest.retention_scan.is_some() {
        return Ok(true);
    }
    let cutoff_nanos = (now - chrono::Duration::days(180))
        .timestamp_nanos_opt()
        .context("180-day retention cutoff is outside the nanosecond range")?;
    if manifest
        .oldest_event_time_unix_nano
        .is_some_and(|oldest| oldest < cutoff_nanos)
    {
        return Ok(true);
    }
    let receipt_cutoff_nanos = (now
        - chrono::Duration::seconds(
            crate::journal::domain::idempotency_window::IDEMPOTENCY_WINDOW_SECONDS,
        ))
    .timestamp_nanos_opt()
    .context("dedupe receipt cleanup cutoff is outside the nanosecond range")?;
    committed_has_expired_dedupe_receipt(&manifest, receipt_cutoff_nanos)
}
