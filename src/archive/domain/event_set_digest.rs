//! The order-free digests of an archived event set's IDs and contents
//! (xor-sha256-v1).

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::shared_kernel::stored_event::StoredEvent;

pub(in crate::archive) fn include_event_id(accumulator: &mut [u8; 32], event_id: &str) {
    let digest: [u8; 32] = Sha256::digest(event_id.as_bytes()).into();
    for (slot, byte) in accumulator.iter_mut().zip(digest) {
        *slot ^= byte;
    }
}

pub(in crate::archive) fn include_event_content(
    accumulator: &mut [u8; 32],
    event: &StoredEvent,
) -> Result<()> {
    let encoded =
        serde_json::to_vec(&event.event).context("encode archive event content digest")?;
    let digest: [u8; 32] = Sha256::digest(encoded).into();
    for (slot, byte) in accumulator.iter_mut().zip(digest) {
        *slot ^= byte;
    }
    Ok(())
}

pub(in crate::archive) fn decode_event_id_digest(encoded: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(encoded).context("decode committed archive event ID digest")?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("committed archive event ID digest must be 32 bytes"))
}

pub(in crate::archive) fn decode_event_content_digest(encoded: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(encoded).context("decode committed archive event content digest")?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("committed archive event content digest must be 32 bytes"))
}
