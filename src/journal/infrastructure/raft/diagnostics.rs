//! Encoding and decoding replicated commands for diagnostics and tests.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::journal::domain::sift_command::SiftCommandV1;
use crate::journal::infrastructure::raft::command_codec::decode_command;
use crate::EventEnvelope;

#[doc(hidden)]
pub fn encode_raft_batch_for_diagnostics(events: Vec<EventEnvelope>) -> Result<Vec<u8>> {
    SiftCommandV1::append_events_now(events).encoded_compressed()
}

#[doc(hidden)]
pub fn encode_default_raft_batch_for_diagnostics(events: Vec<EventEnvelope>) -> Result<Vec<u8>> {
    SiftCommandV1::append_events_now(events).encoded()
}

#[doc(hidden)]
pub fn encode_raft_batch_at_for_diagnostics(
    events: Vec<EventEnvelope>,
    acknowledged_at: &str,
) -> Result<Vec<u8>> {
    let acknowledged_at = DateTime::parse_from_rfc3339(acknowledged_at)
        .context("diagnostic Raft acknowledgement time must be RFC3339")?
        .with_timezone(&Utc);
    SiftCommandV1::append_events_at(events, acknowledged_at).encoded()
}

#[doc(hidden)]
pub fn decode_raft_batch_for_diagnostics(bytes: &[u8]) -> Result<Vec<EventEnvelope>> {
    match decode_command(bytes)? {
        SiftCommandV1::AppendEvents { events, .. } => Ok(events),
        SiftCommandV1::ArchiveCheckpointBarrier { .. } => {
            bail!("Sift Raft command is an archive checkpoint barrier")
        }
        SiftCommandV1::RetentionFence { .. } => bail!("Sift Raft command is a retention fence"),
        SiftCommandV1::ClearRetentionFence { .. } => {
            bail!("Sift Raft command clears a retention fence")
        }
    }
}

#[doc(hidden)]
pub fn encode_archive_checkpoint_barrier_for_diagnostics(
    retention_generation: u64,
    manifest_uri: String,
    manifest_sha256: String,
) -> Result<Vec<u8>> {
    SiftCommandV1::ArchiveCheckpointBarrier {
        retention_generation,
        manifest_uri,
        manifest_sha256,
    }
    .encoded()
}

#[doc(hidden)]
pub fn encode_clear_retention_fence_for_diagnostics(retention_generation: u64) -> Result<Vec<u8>> {
    SiftCommandV1::clear_retention_fence(retention_generation).encoded()
}
