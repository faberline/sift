//! The resident checkpoint record: writing, reading, validating and restoring
//! it.

use std::io::{Read, Write};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::journal::domain::append_policy::validate_retention_fence;
use crate::journal::domain::retention_fence::RetentionFenceV1;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::journal::infrastructure::raft::archive_checkpoint::{
    ARCHIVE_CHECKPOINT_HEADER_BYTES, MAX_ARCHIVE_CHECKPOINT_BYTES,
};
use crate::journal::infrastructure::raft::snapshot_format::{read_one_or_eof, SnapshotMetadata};

pub(super) const RESIDENT_CHECKPOINT_MAGIC: &[u8; 8] = b"SIFTRSD1";

pub(super) const RESIDENT_CHECKPOINT_FORMAT_VERSION: u16 = 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResidentCheckpointV1 {
    pub(super) format_version: u16,
    pub(super) applied_index: u64,
    pub(super) raw_cursor: u64,
    pub(super) event_count: u64,
    pub(super) retention_generation: u64,
    pub(super) event_content_sha256: String,
    pub(super) pending_retention: Option<RetentionFenceV1>,
}

pub(super) fn write_resident_checkpoint(
    checkpoint: &ResidentCheckpointV1,
    writer: &mut dyn Write,
) -> Result<()> {
    validate_resident_checkpoint(checkpoint)?;
    let payload = serde_json::to_vec(checkpoint).context("encode Sift resident checkpoint")?;
    if payload.is_empty() || payload.len() > MAX_ARCHIVE_CHECKPOINT_BYTES {
        bail!("Sift resident checkpoint payload has an invalid length");
    }
    let payload_len =
        u32::try_from(payload.len()).context("resident checkpoint length exceeds u32")?;
    writer
        .write_all(RESIDENT_CHECKPOINT_MAGIC)
        .context("write Sift resident checkpoint magic")?;
    writer
        .write_all(&payload_len.to_le_bytes())
        .context("write Sift resident checkpoint length")?;
    writer
        .write_all(&crc32fast::hash(&payload).to_le_bytes())
        .context("write Sift resident checkpoint checksum")?;
    writer
        .write_all(&payload)
        .context("write Sift resident checkpoint payload")
}

pub(super) fn read_resident_checkpoint(reader: &mut dyn Read) -> Result<ResidentCheckpointV1> {
    let mut header = [0_u8; ARCHIVE_CHECKPOINT_HEADER_BYTES];
    reader
        .read_exact(&mut header)
        .context("read Sift resident checkpoint header")?;
    if &header[..8] != RESIDENT_CHECKPOINT_MAGIC {
        bail!("invalid Sift resident checkpoint magic");
    }
    let payload_len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    let expected_checksum = u32::from_le_bytes(header[12..16].try_into().unwrap());
    if payload_len == 0 || payload_len > MAX_ARCHIVE_CHECKPOINT_BYTES {
        bail!("Sift resident checkpoint payload has an invalid length");
    }
    let mut payload = vec![0_u8; payload_len];
    reader
        .read_exact(&mut payload)
        .context("read Sift resident checkpoint payload")?;
    if crc32fast::hash(&payload) != expected_checksum {
        bail!("Sift resident checkpoint checksum mismatch");
    }
    if read_one_or_eof(reader)?.is_some() {
        bail!("Sift resident checkpoint contains trailing bytes");
    }
    let checkpoint: ResidentCheckpointV1 =
        serde_json::from_slice(&payload).context("decode Sift resident checkpoint")?;
    validate_resident_checkpoint(&checkpoint)?;
    Ok(checkpoint)
}

pub(super) fn validate_resident_checkpoint(checkpoint: &ResidentCheckpointV1) -> Result<()> {
    if checkpoint.format_version != RESIDENT_CHECKPOINT_FORMAT_VERSION
        || checkpoint.applied_index == 0
        || checkpoint.raw_cursor == 0
        || checkpoint.event_count == 0
        || checkpoint.event_count > checkpoint.raw_cursor
        || checkpoint.event_content_sha256.len() != 64
        || !checkpoint
            .event_content_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("Sift resident checkpoint metadata is invalid");
    }
    if let Some(fence) = &checkpoint.pending_retention {
        validate_retention_fence(fence)?;
    }
    Ok(())
}

pub(super) fn restore_resident_checkpoint(
    journal: &DurableJournal,
    checkpoint: &ResidentCheckpointV1,
) -> Result<SnapshotMetadata> {
    if journal.last_cursor() < checkpoint.raw_cursor {
        bail!(
            "Sift local journal cursor {} is behind resident checkpoint cursor {}",
            journal.last_cursor(),
            checkpoint.raw_cursor
        );
    }
    let (local_prefix_events, local_prefix_digest, retention_generation) =
        journal.checkpoint_identity(checkpoint.raw_cursor)?;
    if retention_generation != checkpoint.retention_generation {
        bail!("Sift resident checkpoint retention generation disagrees with local journal");
    }
    let suffix_events = journal.last_cursor().saturating_sub(checkpoint.raw_cursor);
    let expected_events = checkpoint
        .event_count
        .checked_add(suffix_events)
        .context("resident checkpoint event count exhausted u64")?;
    if journal.total_event_count() != expected_events
        || local_prefix_events != checkpoint.event_count
        || hex::encode(local_prefix_digest) != checkpoint.event_content_sha256
    {
        bail!("Sift resident checkpoint event count disagrees with local journal");
    }
    Ok(SnapshotMetadata {
        applied_index: checkpoint.applied_index,
        last_cursor: checkpoint.raw_cursor,
        event_count: checkpoint.event_count,
        pending_retention: Some(checkpoint.pending_retention.clone()),
    })
}
