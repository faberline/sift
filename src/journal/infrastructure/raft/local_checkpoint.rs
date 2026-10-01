//! The local checkpoint record: writing, reading, validating and restoring it.

use std::io::{Read, Write};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::journal::domain::append_policy::validate_retention_fence;
use crate::journal::domain::journal_api::JournalApi;
type DurableJournal<'a> = dyn JournalApi<Error = anyhow::Error> + 'a;
use crate::journal::infrastructure::raft::checkpoint_framing::{CheckpointFrame, FrameErrors};
use crate::journal::infrastructure::raft::snapshot_format::SnapshotMetadata;
use crate::shared_kernel::retention_fence::RetentionFenceV1;

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

const FRAME: CheckpointFrame = CheckpointFrame {
    magic: LOCAL_CHECKPOINT_MAGIC,
    errors: FrameErrors {
        encode: "encode Sift local checkpoint",
        invalid_length: "Sift local checkpoint payload has an invalid length",
        length_overflow: "local checkpoint length exceeds u32",
        write_magic: "write Sift local checkpoint magic",
        write_length: "write Sift local checkpoint length",
        write_checksum: "write Sift local checkpoint checksum",
        write_payload: "write Sift local checkpoint payload",
        read_header: "read Sift local checkpoint header",
        invalid_magic: "invalid Sift local checkpoint magic",
        read_payload: "read Sift local checkpoint payload",
        checksum_mismatch: "Sift local checkpoint checksum mismatch",
        trailing_bytes: "Sift local checkpoint contains trailing bytes",
        decode: "decode Sift local checkpoint",
    },
};

pub(super) fn write_local_checkpoint(
    checkpoint: &LocalCheckpointV1,
    writer: &mut dyn Write,
) -> Result<()> {
    validate_local_checkpoint(checkpoint)?;
    FRAME.write(checkpoint, writer)
}

pub(super) fn read_local_checkpoint(reader: &mut dyn Read) -> Result<LocalCheckpointV1> {
    let checkpoint = FRAME.read(reader)?;
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
    journal: &DurableJournal<'_>,
    checkpoint: &LocalCheckpointV1,
) -> Result<SnapshotMetadata> {
    if journal.last_cursor() < checkpoint.raw_cursor {
        bail!(
            "Sift local journal cursor {} is behind checkpoint cursor {}",
            journal.last_cursor(),
            checkpoint.raw_cursor
        );
    }
    let receipt = journal.archive_maintenance().archive_local(journal)?;
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
