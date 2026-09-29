//! Splitting governed events into Raft batches under the item and byte limits.

use anyhow::{Context, Result};

use crate::event::EventEnvelope;

pub(in crate::ingest) fn split_governed_batches(
    events: Vec<EventEnvelope>,
) -> Result<Vec<Vec<EventEnvelope>>> {
    let empty_size =
        crate::journal::domain::sift_command::SiftCommandV1::append_events_size_bound(Vec::new())
            .uncompressed_len()?;
    let mut chunks = Vec::new();
    let mut batch = Vec::new();
    let mut encoded_size = empty_size;
    for event in events {
        let event_size = serde_json::to_vec(&event)
            .context("encode governed Sift event for Raft batching")?
            .len();
        let separator = usize::from(!batch.is_empty());
        if !batch.is_empty()
            && (batch.len() >= crate::journal::domain::raft_batch_limits::RAFT_BATCH_MAX_ITEMS
                || encoded_size + separator + event_size
                    > crate::journal::domain::raft_batch_limits::RAFT_BATCH_MAX_BYTES)
        {
            chunks.push(std::mem::take(&mut batch));
            encoded_size = empty_size;
        }
        encoded_size += usize::from(!batch.is_empty()) + event_size;
        batch.push(event);
    }
    if !batch.is_empty() {
        chunks.push(batch);
    }
    Ok(chunks)
}
