//! Folding an event's encoded content into the journal's content digest.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::event::EventEnvelope;

pub(in crate::journal) fn xor_event_content_digest(
    accumulator: &mut [u8; 32],
    event: &EventEnvelope,
) -> Result<()> {
    let encoded = serde_json::to_vec(event).context("encode Sift event content digest")?;
    let digest: [u8; 32] = Sha256::digest(encoded).into();
    for (slot, byte) in accumulator.iter_mut().zip(digest) {
        *slot ^= byte;
    }
    Ok(())
}
