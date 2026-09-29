//! Validating an archive manifest and its dedupe receipts before they are
//! trusted.

use std::collections::BTreeSet;

use anyhow::{bail, Context, Result};

use crate::archive::domain::archive_manifest::{
    ArchiveDedupeReceipt, ArchiveManifest, ARCHIVE_FORMAT_VERSION,
};
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;

pub(in crate::archive) fn validate_dedupe_receipt(receipt: &ArchiveDedupeReceipt) -> Result<()> {
    if !receipt.object_uri.starts_with("gs://")
        || receipt.bytes == 0
        || receipt.sha256.len() != 64
        || !receipt.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || receipt.entry_count == 0
        || receipt.first_cursor == 0
        || receipt.first_cursor > receipt.last_cursor
        || receipt.min_acknowledged_at_unix_nano > receipt.max_acknowledged_at_unix_nano
    {
        bail!("archive dedupe receipt has invalid metadata");
    }
    Ok(())
}

pub(crate) fn validate_archive_manifest(manifest: &ArchiveManifest) -> Result<()> {
    if manifest.format_version != ARCHIVE_FORMAT_VERSION
        || manifest.source_cluster_id.trim().is_empty()
        || manifest.source_node_id.trim().is_empty()
        || manifest.event_id_digest_algorithm != "xor-sha256-v1"
        || manifest.event_id_sha256.len() != 64
        || !manifest
            .event_id_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || manifest.event_content_digest_algorithm != "xor-sha256-v1"
        || manifest.event_content_sha256.len() != 64
        || !manifest
            .event_content_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !manifest.catalog_uri.starts_with("gs://")
        || manifest.catalog_root.entry_count
            != manifest
                .segment_count
                .saturating_add(manifest.blob_count)
                .saturating_add(manifest.dedupe_receipt_count)
        || (manifest.segment_count == 0) != manifest.oldest_event_time_unix_nano.is_none()
        || manifest.catalog_root.page_bytes_limit as usize
            != storage_segment::DEFAULT_CATALOG_PAGE_BYTES
        || manifest.gc_plan_uri.is_some() != manifest.gc_plan_root.is_some()
        || manifest
            .gc_plan_uri
            .as_ref()
            .is_some_and(|uri| !uri.starts_with("gs://"))
        || (manifest.gc_object_count == 0) != manifest.gc_plan_root.is_none()
        || manifest
            .gc_plan_root
            .as_ref()
            .is_some_and(|root| root.entry_count != manifest.gc_object_count)
    {
        bail!("Sift archive manifest has invalid identity or digest fields");
    }
    if serde_json::to_vec(manifest)?.len() >= 64 * 1024 {
        bail!("Sift archive root manifest exceeds 64 KiB");
    }
    if manifest.watermarks.max_cursor() > manifest.raft_snapshot_index
        || manifest.event_count > manifest.raft_snapshot_index
        || manifest.retained_watermarks.logs > manifest.watermarks.logs
        || manifest.retained_watermarks.metrics > manifest.watermarks.metrics
        || manifest.retained_watermarks.traces > manifest.watermarks.traces
    {
        bail!("Sift archive manifest has invalid coverage or retained watermarks");
    }
    if let Some(delta) = &manifest.retention_delta {
        if !delta.source_manifest_uri.starts_with("gs://")
            || delta.source_manifest_sha256.len() != 64
            || !delta
                .source_manifest_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || delta.source_event_content_sha256.len() != 64
            || !delta
                .source_event_content_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || manifest.retention_generation != delta.source_generation.saturating_add(1)
            || manifest.event_count > delta.source_event_count
        {
            bail!("Sift archive retention delta has invalid source metadata");
        }
    }
    if let Some(scan) = &manifest.retention_scan {
        if scan.after_catalog_key.as_str() <= "segment/"
            || !scan.after_catalog_key.starts_with("segment/")
        {
            bail!("Sift archive retention scan has an invalid catalog cursor");
        }
        if manifest
            .retention_delta
            .as_ref()
            .is_none_or(|delta| delta.cutoff_unix_nano != scan.cutoff_unix_nano)
        {
            bail!("Sift archive retention scan lacks its generation delta");
        }
    }
    let mut event_count = 0_u64;
    let mut retained_watermarks = ArchiveWatermarks::default();
    let mut segment_ids = BTreeSet::new();
    for segment in &manifest.segments {
        if !segment.object_uri.starts_with("gs://")
            || segment.parquet_bytes == 0
            || segment.parquet_sha256.len() != 64
            || !segment
                .parquet_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || segment.source.segment_id.trim().is_empty()
            || segment.source.event_count == 0
            || segment.source.first_cursor == 0
            || segment.source.first_cursor > segment.source.last_cursor
            || segment.source.last_cursor > manifest.watermarks.through(segment.signal)
            || segment.source.min_event_time_unix_nano > segment.source.max_event_time_unix_nano
            || segment.min_acknowledged_at_unix_nano > segment.max_acknowledged_at_unix_nano
            || validate_dedupe_receipt(&segment.dedupe_receipt).is_err()
            || segment.dedupe_receipt.entry_count != segment.source.event_count
            || segment.dedupe_receipt.first_cursor != segment.source.first_cursor
            || segment.dedupe_receipt.last_cursor != segment.source.last_cursor
            || segment.dedupe_receipt.min_acknowledged_at_unix_nano
                != segment.min_acknowledged_at_unix_nano
            || segment.dedupe_receipt.max_acknowledged_at_unix_nano
                != segment.max_acknowledged_at_unix_nano
            || !segment_ids.insert(segment.source.segment_id.clone())
        {
            bail!("Sift archive manifest has an invalid segment object");
        }
        event_count = event_count
            .checked_add(segment.source.event_count)
            .context("Sift archive manifest event count exhausted u64")?;
        retained_watermarks.include(segment.signal, segment.source.last_cursor);
    }
    if !manifest.segments.is_empty() || manifest.segment_count == 0 {
        let observed_oldest = manifest
            .segments
            .iter()
            .map(|segment| segment.source.min_event_time_unix_nano)
            .min();
        let oldest_matches = match manifest.retention_scan {
            Some(_) => match (manifest.oldest_event_time_unix_nano, observed_oldest) {
                (Some(lower_bound), Some(observed)) => lower_bound <= observed,
                (None, None) => true,
                _ => false,
            },
            None => observed_oldest == manifest.oldest_event_time_unix_nano,
        };
        if event_count != manifest.event_count
            || retained_watermarks != manifest.retained_watermarks
            || !oldest_matches
            || manifest.segments.len() as u64 != manifest.segment_count
        {
            bail!("Sift archive manifest segment totals do not match retained event metadata");
        }
    }
    let mut blob_hashes = BTreeSet::new();
    for blob in &manifest.blobs {
        if !blob.object_uri.starts_with("gs://")
            || blob.reference.size == 0
            || !blob_hashes.insert(blob.reference.hash.clone())
        {
            bail!("Sift archive manifest has an invalid blob object");
        }
    }
    if (!manifest.blobs.is_empty() || manifest.blob_count == 0)
        && manifest.blobs.len() as u64 != manifest.blob_count
    {
        bail!("Sift archive manifest blob count does not match its catalog root");
    }
    let mut prior_gc_uri: Option<&str> = None;
    for uri in &manifest.gc_object_uris {
        if !uri.starts_with("gs://") || prior_gc_uri.is_some_and(|prior| prior >= uri.as_str()) {
            bail!("Sift archive manifest has an invalid or unsorted GC object URI");
        }
        service_backup::GcsSink::from_exact_uri(uri)
            .with_context(|| format!("validate archive manifest GC object URI {uri}"))?;
        prior_gc_uri = Some(uri);
    }
    if (!manifest.gc_object_uris.is_empty() || manifest.gc_object_count == 0)
        && manifest.gc_object_uris.len() as u64 != manifest.gc_object_count
    {
        bail!("Sift archive manifest GC count does not match its catalog root");
    }
    Ok(())
}
