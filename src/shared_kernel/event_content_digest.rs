//! The event-content digest: the XOR of each event's SHA-256, so a prefix's
//! digest and its suffix's combine without rereading either.

use anyhow::{Context, Result};

pub(crate) fn xor_digest(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut combined = left;
    for (slot, byte) in combined.iter_mut().zip(right) {
        *slot ^= byte;
    }
    combined
}

pub(crate) fn decode_digest(value: &str) -> Result<[u8; 32]> {
    hex::decode(value)
        .context("decode Sift SHA-256 digest")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Sift SHA-256 digest must be 32 bytes"))
}
