//! Segment file helpers: framed event reads, checksums, verification, and
//! routes recovered from segment paths.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::DateTime;
use sha2::{Digest, Sha256};

use crate::journal::domain::segment_manifest::SegmentManifest;
use crate::shared_kernel::stored_event::StoredEvent;

pub(super) fn read_events(path: &Path) -> Result<Vec<StoredEvent>> {
    storage_durable::FramedLogReader::read_frames(path, 0)?
        .into_iter()
        .map(|frame| {
            let event: StoredEvent = serde_json::from_slice(&frame.payload)?;
            if event.cursor != frame.seq {
                bail!(
                    "segment frame {} contains cursor {} in {}",
                    frame.seq,
                    event.cursor,
                    path.display()
                );
            }
            Ok(event)
        })
        .collect()
}

pub(super) fn event_time_unix_nano(event: &StoredEvent) -> Result<i64> {
    DateTime::parse_from_rfc3339(&event.event.occurred_at)
        .context("segment event occurred_at must be RFC3339")?
        .timestamp_nanos_opt()
        .context("segment event occurred_at is outside the nanosecond range")
}

pub(super) fn read_events_after(path: &Path, after: u64, limit: usize) -> Result<Vec<StoredEvent>> {
    storage_durable::FramedLogReader::read_frames_bounded(path, after, limit)?
        .into_iter()
        .map(|frame| {
            let event: StoredEvent = serde_json::from_slice(&frame.payload)?;
            if event.cursor != frame.seq {
                bail!(
                    "segment frame {} contains cursor {} in {}",
                    frame.seq,
                    event.cursor,
                    path.display()
                );
            }
            Ok(event)
        })
        .collect()
}

pub(super) fn sha256_file(path: &Path) -> Result<String> {
    Ok(hex::encode(Sha256::digest(fs::read(path)?)))
}

pub(super) fn verify_segment(manifest: &SegmentManifest) -> Result<()> {
    let bytes = fs::metadata(&manifest.local_path)
        .with_context(|| format!("stat segment {}", manifest.local_path.display()))?
        .len();
    if bytes != manifest.bytes || sha256_file(&manifest.local_path)? != manifest.sha256 {
        bail!(
            "sealed segment {} failed size/hash verification",
            manifest.segment_id
        );
    }
    Ok(())
}

pub(super) fn route_from_path(path: &Path) -> Result<(u64, u16)> {
    let shard = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .and_then(|value| value.strip_prefix("shard-"))
        .context("segment path lacks shard directory")?
        .parse()?;
    let epoch = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .and_then(|value| value.strip_prefix("epoch-"))
        .context("segment path lacks epoch directory")?
        .parse()?;
    Ok((epoch, shard))
}

pub(super) fn files_with_extension(root: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut output = Vec::new();
    if !root.exists() {
        return Ok(output);
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else if entry.path().extension().and_then(|value| value.to_str()) == Some(extension) {
                output.push(entry.path());
            }
        }
    }
    output.sort();
    Ok(output)
}
