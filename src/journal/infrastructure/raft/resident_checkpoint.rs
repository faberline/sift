//! The resident checkpoint record: writing, reading, validating and restoring
//! it.

use std::io::{Read, Write};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::journal::domain::append_policy::validate_retention_fence;
use crate::journal::domain::journal_api::JournalApi;
type DurableJournal<'a> = dyn JournalApi<Error = anyhow::Error> + 'a;
use crate::journal::infrastructure::raft::checkpoint_framing::{CheckpointFrame, FrameErrors};
use crate::journal::infrastructure::raft::snapshot_format::SnapshotMetadata;
use crate::shared_kernel::retention_fence::RetentionFenceV1;

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

const FRAME: CheckpointFrame = CheckpointFrame {
    magic: RESIDENT_CHECKPOINT_MAGIC,
    errors: FrameErrors {
        encode: "encode Sift resident checkpoint",
        invalid_length: "Sift resident checkpoint payload has an invalid length",
        length_overflow: "resident checkpoint length exceeds u32",
        write_magic: "write Sift resident checkpoint magic",
        write_length: "write Sift resident checkpoint length",
        write_checksum: "write Sift resident checkpoint checksum",
        write_payload: "write Sift resident checkpoint payload",
        read_header: "read Sift resident checkpoint header",
        invalid_magic: "invalid Sift resident checkpoint magic",
        read_payload: "read Sift resident checkpoint payload",
        checksum_mismatch: "Sift resident checkpoint checksum mismatch",
        trailing_bytes: "Sift resident checkpoint contains trailing bytes",
        decode: "decode Sift resident checkpoint",
    },
};

pub(super) fn write_resident_checkpoint(
    checkpoint: &ResidentCheckpointV1,
    writer: &mut dyn Write,
) -> Result<()> {
    validate_resident_checkpoint(checkpoint)?;
    FRAME.write(checkpoint, writer)
}

pub(super) fn read_resident_checkpoint(reader: &mut dyn Read) -> Result<ResidentCheckpointV1> {
    let checkpoint = FRAME.read(reader)?;
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
    journal: &DurableJournal<'_>,
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
