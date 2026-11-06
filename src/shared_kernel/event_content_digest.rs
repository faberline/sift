//! The event-content digest: the XOR of each event's SHA-256, so a prefix's
//! digest and its suffix's combine without rereading either.

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::shared_kernel::event::EventEnvelope;

pub(crate) fn event_content_digest(event: &EventEnvelope) -> Result<[u8; 32], serde_json::Error> {
    let encoded = serde_json::to_vec(event)?;
    Ok(Sha256::digest(encoded).into())
}

pub(crate) fn xor_digest_in_place(target: &mut [u8; 32], value: [u8; 32]) {
    for (target, value) in target.iter_mut().zip(value) {
        *target ^= value;
    }
}

pub(crate) fn xor_digest(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let mut combined = left;
    xor_digest_in_place(&mut combined, right);
    combined
}

#[derive(Debug, Error)]
pub(crate) enum DigestError {
    #[error("decode Sift SHA-256 digest")]
    Hex(#[source] hex::FromHexError),
    #[error("Sift SHA-256 digest must be 32 bytes")]
    Length,
}

pub(crate) fn decode_digest(value: &str) -> Result<[u8; 32], DigestError> {
    hex::decode(value)
        .map_err(DigestError::Hex)?
        .try_into()
        .map_err(|_| DigestError::Length)
}

#[cfg(test)]
mod tests {
    use super::{event_content_digest, xor_digest, xor_digest_in_place};
    use crate::shared_kernel::stored_event::StoredEvent;

    #[test]
    fn fixed_event_content_digests_keep_the_original_bytes_and_xor() {
        let cases: &[(&[u8], &str)] = &[
            (
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/refactor_compatibility/event-log.json"
                )),
                "40667900c021dd776e3db6b603f62804362cffedc4444fe270e9c09d9add64d5",
            ),
            (
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/refactor_compatibility/event-metric.json"
                )),
                "9629bdf46608eb3ad5c55004017f3fc0992a342c74bf1945ee7bed70fa7cef47",
            ),
            (
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/fixtures/refactor_compatibility/event-span.json"
                )),
                "0bd5cc72c59005fd7064a28c166f9853169d48ed0bab52faec39865b9459b1aa",
            ),
        ];
        let mut forward = [0; 32];
        let mut reverse = [0; 32];
        for (bytes, expected) in cases {
            let stored: StoredEvent = serde_json::from_slice(bytes).unwrap();
            let digest = event_content_digest(&stored.event).unwrap();
            assert_eq!(hex::encode(digest), *expected);
            xor_digest_in_place(&mut forward, digest);
            assert_eq!(xor_digest(digest, digest), [0; 32]);
        }
        for (bytes, _) in cases.iter().rev() {
            let stored: StoredEvent = serde_json::from_slice(bytes).unwrap();
            xor_digest_in_place(&mut reverse, event_content_digest(&stored.event).unwrap());
        }
        assert_eq!(forward, reverse);
    }
}
