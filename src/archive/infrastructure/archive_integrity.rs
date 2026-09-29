//! Checking archived segments and files against their recorded hashes.

use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::DateTime;
use sha2::{Digest, Sha256};
use std::io::Read;

use crate::archive::domain::archive_event_time::acknowledgement_time_bounds;
use crate::archive::domain::archive_manifest::ArchiveSegment;
use crate::archive::infrastructure::parquet_event_codec::{
    decode_parquet_batch, open_parquet_reader,
};
use crate::shared_kernel::stored_event::StoredEvent;

pub(super) fn verify_archive_segment_file(segment: &ArchiveSegment, path: &Path) -> Result<()> {
    let mut reader = open_parquet_reader(path)?;
    let mut event_count = 0_u64;
    let mut first_cursor = None;
    let mut last_cursor = None;
    let mut minimum_acknowledged = None::<i64>;
    let mut maximum_acknowledged = None::<i64>;
    for batch in &mut reader {
        for event in decode_parquet_batch(&batch?)? {
            if last_cursor.is_some_and(|previous| event.cursor <= previous)
                || event.event.signal != segment.signal
            {
                bail!(
                    "archive segment {} does not match its committed metadata",
                    segment.source.segment_id
                );
            }
            let acknowledged = DateTime::parse_from_rfc3339(&event.acknowledged_at)
                .context("archive event acknowledged_at must be RFC3339")?
                .timestamp_nanos_opt()
                .context("archive acknowledgement time is outside the nanosecond range")?;
            first_cursor.get_or_insert(event.cursor);
            last_cursor = Some(event.cursor);
            minimum_acknowledged = Some(
                minimum_acknowledged
                    .map(|minimum| minimum.min(acknowledged))
                    .unwrap_or(acknowledged),
            );
            maximum_acknowledged = Some(
                maximum_acknowledged
                    .map(|maximum| maximum.max(acknowledged))
                    .unwrap_or(acknowledged),
            );
            event_count = event_count.saturating_add(1);
        }
    }
    if event_count != segment.source.event_count
        || first_cursor != Some(segment.source.first_cursor)
        || last_cursor != Some(segment.source.last_cursor)
        || (minimum_acknowledged, maximum_acknowledged)
            != (
                Some(segment.min_acknowledged_at_unix_nano),
                Some(segment.max_acknowledged_at_unix_nano),
            )
    {
        bail!(
            "archive segment {} does not match its committed metadata",
            segment.source.segment_id
        );
    }
    Ok(())
}

pub(in crate::archive) fn verify_archive_segment(
    segment: &ArchiveSegment,
    events: &[StoredEvent],
) -> Result<()> {
    let acknowledgement_bounds = acknowledgement_time_bounds(events)?;
    if events.len() as u64 != segment.source.event_count
        || events.first().map(|event| event.cursor) != Some(segment.source.first_cursor)
        || events.last().map(|event| event.cursor) != Some(segment.source.last_cursor)
        || events
            .windows(2)
            .any(|pair| pair[0].cursor >= pair[1].cursor)
        || events
            .iter()
            .any(|event| event.event.signal != segment.signal)
        || acknowledgement_bounds
            != (
                segment.min_acknowledged_at_unix_nano,
                segment.max_acknowledged_at_unix_nano,
            )
    {
        bail!(
            "archive segment {} does not match its committed metadata",
            segment.source.segment_id
        );
    }
    Ok(())
}

pub(super) fn verify_bytes(
    expected_hash: &str,
    expected_size: u64,
    bytes: &[u8],
    kind: &str,
) -> Result<()> {
    if bytes.len() as u64 != expected_size || sha256(bytes) != expected_hash {
        bail!("{kind} archive object failed hash/size verification");
    }
    Ok(())
}

pub(super) fn verify_file(
    path: &Path,
    expected_hash: &str,
    expected_size: u64,
    kind: &str,
) -> Result<()> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("open {kind} {}", path.display()))?;
    if file.metadata()?.len() != expected_size {
        bail!("{kind} archive object failed hash/size verification");
    }
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    if hex::encode(digest.finalize()) != expected_hash {
        bail!("{kind} archive object failed hash/size verification");
    }
    Ok(())
}

pub(in crate::archive) fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
