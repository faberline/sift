//! The order-free digests of an archived event set's IDs and contents
//! (xor-sha256-v1).

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::shared_kernel::stored_event::StoredEvent;

#[derive(Debug, Error)]
pub(in crate::archive) enum EventSetDigestError {
    #[error("encode archive event content digest")]
    EncodeContent(#[source] serde_json::Error),
    #[error("decode committed archive event ID digest")]
    DecodeIdDigest(#[source] hex::FromHexError),
    #[error("committed archive event ID digest must be 32 bytes")]
    IdDigestLength,
    #[error("decode committed archive event content digest")]
    DecodeContentDigest(#[source] hex::FromHexError),
    #[error("committed archive event content digest must be 32 bytes")]
    ContentDigestLength,
}

pub(in crate::archive) fn include_event_id(accumulator: &mut [u8; 32], event_id: &str) {
    let digest: [u8; 32] = Sha256::digest(event_id.as_bytes()).into();
    for (slot, byte) in accumulator.iter_mut().zip(digest) {
        *slot ^= byte;
    }
}

pub(in crate::archive) fn include_event_content(
    accumulator: &mut [u8; 32],
    event: &StoredEvent,
) -> Result<(), EventSetDigestError> {
    let encoded = serde_json::to_vec(&event.event).map_err(EventSetDigestError::EncodeContent)?;
    let digest: [u8; 32] = Sha256::digest(encoded).into();
    for (slot, byte) in accumulator.iter_mut().zip(digest) {
        *slot ^= byte;
    }
    Ok(())
}

pub(in crate::archive) fn decode_event_id_digest(
    encoded: &str,
) -> Result<[u8; 32], EventSetDigestError> {
    let bytes = hex::decode(encoded).map_err(EventSetDigestError::DecodeIdDigest)?;
    bytes
        .try_into()
        .map_err(|_| EventSetDigestError::IdDigestLength)
}

pub(in crate::archive) fn decode_event_content_digest(
    encoded: &str,
) -> Result<[u8; 32], EventSetDigestError> {
    let bytes = hex::decode(encoded).map_err(EventSetDigestError::DecodeContentDigest)?;
    bytes
        .try_into()
        .map_err(|_| EventSetDigestError::ContentDigestLength)
}
