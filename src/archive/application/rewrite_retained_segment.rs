//! Rewriting a segment that retention expires only in part: a new Parquet
//! object with the retained events and a dedupe receipt of its own.

use crate::archive::application::archive_services::ArchiveServices;

use std::path::PathBuf;

use anyhow::Result;

use crate::archive::domain::archive_event_time::{
    acknowledgement_time_bounds, event_time_unix_nano,
};
use crate::archive::infrastructure::dedupe_receipt_archive::archive_dedupe_receipts;
use crate::archive::infrastructure::gcs_uri::gcs_uri;
use crate::shared_kernel::archive_manifest::ArchiveSegment;
use crate::shared_kernel::sha256_hex::sha256_hex;
use crate::shared_kernel::stored_event::StoredEvent;

/// Write a replacement for a segment that keeps only its retained events,
/// with its own dedupe receipt, into a retention pass's archive
/// transaction, and return the replacement's manifest segment.
pub(super) fn rewrite_retained_segment(
    services: &ArchiveServices,
    transaction: &mut storage_segment::ArchiveTransaction,
    segment: &ArchiveSegment,
    retained: &[StoredEvent],
    bucket: &str,
    rewrite_prefix: &str,
) -> Result<ArchiveSegment> {
    let parquet = services.encode_parquet(retained)?;
    let parquet_sha256 = sha256_hex(&parquet);
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
    let (minimum_acknowledged_at, maximum_acknowledged_at) = acknowledgement_time_bounds(retained)?;
    let dedupe_receipt = archive_dedupe_receipts(transaction, bucket, rewrite_prefix, retained)?;
    Ok(ArchiveSegment {
        signal: segment.signal,
        source,
        object_uri: gcs_uri(bucket, &key),
        parquet_bytes: parquet.len() as u64,
        parquet_sha256,
        min_acknowledged_at_unix_nano: minimum_acknowledged_at,
        max_acknowledged_at_unix_nano: maximum_acknowledged_at,
        dedupe_receipt,
    })
}
