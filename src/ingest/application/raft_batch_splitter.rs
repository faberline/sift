//! Splitting governed events into Raft batches under the item and byte limits.

use anyhow::{Context, Result};

use crate::shared_kernel::event::EventEnvelope;

pub(in crate::ingest) fn split_governed_batches(
    events: Vec<EventEnvelope>,
    empty_size: usize,
    maximum_items: usize,
    maximum_bytes: usize,
) -> Result<Vec<Vec<EventEnvelope>>> {
    let mut chunks = Vec::new();
    let mut batch = Vec::new();
    let mut encoded_size = empty_size;
    for event in events {
        let event_size = serde_json::to_vec(&event)
            .context("encode governed Sift event for Raft batching")?
            .len();
        let separator = usize::from(!batch.is_empty());
        if !batch.is_empty()
            && (batch.len() >= maximum_items
                || encoded_size + separator + event_size > maximum_bytes)
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
