//! The local checkpoint record: writing, reading, validating and restoring it.

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

pub(super) const LOCAL_CHECKPOINT_MAGIC: &[u8; 8] = b"SIFTLCP1";

pub(super) const LOCAL_CHECKPOINT_FORMAT_VERSION: u16 = 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LocalCheckpointV1 {
    pub(super) format_version: u16,
    pub(super) applied_index: u64,
    pub(super) raw_cursor: u64,
    pub(super) local_snapshot_index: u64,
    pub(super) watermarks: crate::shared_kernel::archive_watermarks::ArchiveWatermarks,
    pub(super) pending_retention: Option<RetentionFenceV1>,
}

pub(super) fn write_local_checkpoint(
    checkpoint: &LocalCheckpointV1,
    writer: &mut dyn Write,
) -> Result<()> {
    validate_local_checkpoint(checkpoint)?;
    let payload = serde_json::to_vec(checkpoint).context("encode Sift local checkpoint")?;
    if payload.is_empty() || payload.len() > MAX_ARCHIVE_CHECKPOINT_BYTES {
        bail!("Sift local checkpoint payload has an invalid length");
    }
    let payload_len =
        u32::try_from(payload.len()).context("local checkpoint length exceeds u32")?;
    writer
        .write_all(LOCAL_CHECKPOINT_MAGIC)
        .context("write Sift local checkpoint magic")?;
    writer
        .write_all(&payload_len.to_le_bytes())
        .context("write Sift local checkpoint length")?;
    writer
        .write_all(&crc32fast::hash(&payload).to_le_bytes())
        .context("write Sift local checkpoint checksum")?;
    writer
        .write_all(&payload)
        .context("write Sift local checkpoint payload")
}

pub(super) fn read_local_checkpoint(reader: &mut dyn Read) -> Result<LocalCheckpointV1> {
    let mut header = [0_u8; ARCHIVE_CHECKPOINT_HEADER_BYTES];
    reader
        .read_exact(&mut header)
        .context("read Sift local checkpoint header")?;
    if &header[..8] != LOCAL_CHECKPOINT_MAGIC {
        bail!("invalid Sift local checkpoint magic");
    }
    let payload_len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    let expected_checksum = u32::from_le_bytes(header[12..16].try_into().unwrap());
    if payload_len == 0 || payload_len > MAX_ARCHIVE_CHECKPOINT_BYTES {
        bail!("Sift local checkpoint payload has an invalid length");
    }
    let mut payload = vec![0_u8; payload_len];
    reader
        .read_exact(&mut payload)
        .context("read Sift local checkpoint payload")?;
    if crc32fast::hash(&payload) != expected_checksum {
        bail!("Sift local checkpoint checksum mismatch");
    }
    if read_one_or_eof(reader)?.is_some() {
        bail!("Sift local checkpoint contains trailing bytes");
    }
    let checkpoint: LocalCheckpointV1 =
        serde_json::from_slice(&payload).context("decode Sift local checkpoint")?;
    validate_local_checkpoint(&checkpoint)?;
    Ok(checkpoint)
}

pub(super) fn validate_local_checkpoint(checkpoint: &LocalCheckpointV1) -> Result<()> {
    if checkpoint.format_version != LOCAL_CHECKPOINT_FORMAT_VERSION
        || checkpoint.applied_index == 0
        || checkpoint.raw_cursor != checkpoint.local_snapshot_index
        || checkpoint.watermarks.max_cursor() != checkpoint.local_snapshot_index
    {
        bail!("Sift local checkpoint metadata is invalid");
    }
    if let Some(fence) = &checkpoint.pending_retention {
        validate_retention_fence(fence)?;
    }
    Ok(())
}

pub(super) fn restore_local_checkpoint(
    journal: &DurableJournal,
    checkpoint: &LocalCheckpointV1,
) -> Result<SnapshotMetadata> {
    if journal.last_cursor() < checkpoint.raw_cursor {
        bail!(
            "Sift local journal cursor {} is behind checkpoint cursor {}",
            journal.last_cursor(),
            checkpoint.raw_cursor
        );
    }
    let receipt = crate::storage::archive::archive_journal_local(journal)?;
    if receipt.snapshot_index < checkpoint.local_snapshot_index
        || receipt.watermarks.logs < checkpoint.watermarks.logs
        || receipt.watermarks.metrics < checkpoint.watermarks.metrics
        || receipt.watermarks.traces < checkpoint.watermarks.traces
    {
        bail!("Sift local journal does not cover the checkpoint manifest");
    }
    Ok(SnapshotMetadata {
        applied_index: checkpoint.applied_index,
        last_cursor: journal.last_cursor(),
        event_count: journal.total_event_count(),
        pending_retention: Some(checkpoint.pending_retention.clone()),
    })
}
