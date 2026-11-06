//! Folding an event's encoded content into the journal's content digest.

use thiserror::Error;

use crate::shared_kernel::event::EventEnvelope;
use crate::shared_kernel::event_content_digest::{event_content_digest, xor_digest_in_place};

#[derive(Debug, Error)]
#[error("encode Sift event content digest")]
pub(in crate::journal) struct ContentDigestError(#[source] serde_json::Error);

pub(in crate::journal) fn xor_event_content_digest(
    accumulator: &mut [u8; 32],
    event: &EventEnvelope,
) -> Result<(), ContentDigestError> {
    let digest = event_content_digest(event).map_err(ContentDigestError)?;
    xor_digest_in_place(accumulator, digest);
    Ok(())
}
