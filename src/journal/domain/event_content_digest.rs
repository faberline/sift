//! Folding an event's encoded content into the journal's content digest.

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::shared_kernel::event::EventEnvelope;

#[derive(Debug, Error)]
#[error("encode Sift event content digest")]
pub(in crate::journal) struct ContentDigestError(#[source] serde_json::Error);

pub(in crate::journal) fn xor_event_content_digest(
    accumulator: &mut [u8; 32],
    event: &EventEnvelope,
) -> Result<(), ContentDigestError> {
    let encoded = serde_json::to_vec(event).map_err(ContentDigestError)?;
    let digest: [u8; 32] = Sha256::digest(encoded).into();
    for (slot, byte) in accumulator.iter_mut().zip(digest) {
        *slot ^= byte;
    }
    Ok(())
}
