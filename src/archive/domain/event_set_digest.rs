//! The order-free digests of an archived event set's IDs and contents
//! (xor-sha256-v1).

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::shared_kernel::event_content_digest::{event_content_digest, xor_digest_in_place};
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
    xor_digest_in_place(accumulator, digest);
}

pub(in crate::archive) fn include_event_content(
    accumulator: &mut [u8; 32],
    event: &StoredEvent,
) -> Result<(), EventSetDigestError> {
    let digest = event_content_digest(&event.event).map_err(EventSetDigestError::EncodeContent)?;
    xor_digest_in_place(accumulator, digest);
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

#[cfg(test)]
mod tests {
    use super::{include_event_content, include_event_id};
    use crate::shared_kernel::stored_event::StoredEvent;

    #[test]
    fn archive_content_digest_excludes_receipt_metadata() {
        let mut stored: StoredEvent = serde_json::from_slice(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/refactor_compatibility/event-log.json"
        )))
        .unwrap();
        let mut digest = [0; 32];
        include_event_content(&mut digest, &stored).unwrap();
        assert_eq!(
            hex::encode(digest),
            "40667900c021dd776e3db6b603f62804362cffedc4444fe270e9c09d9add64d5"
        );
        stored.cursor = 999;
        stored.acknowledged_at = "2026-10-01T00:00:00Z".into();
        include_event_content(&mut digest, &stored).unwrap();
        assert_eq!(digest, [0; 32]);

        include_event_id(&mut digest, "log-fixed-001");
        assert_eq!(
            hex::encode(digest),
            "96daa04914361172089c7e1a6ebba71bbf2ee82c5c437b1c33f3c0454765877b"
        );
        include_event_id(&mut digest, "log-fixed-001");
        assert_eq!(digest, [0; 32]);
    }
}
