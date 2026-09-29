//! The group-commit batch the journal commits, and the queue and worker that
//! collect ingest requests into it.

use anyhow::Result;

use crate::event::{EventEnvelope, SignalKind};
use crate::AppendResult;

pub(in crate::ingest) struct IngestBatchRequest {
    events: Vec<EventEnvelope>,
    encoded_bytes: usize,
}

impl IngestBatchRequest {
    pub(in crate::ingest) fn new(events: Vec<EventEnvelope>) -> Result<Self> {
        let encoded_bytes =
            crate::durability::SiftCommandV1::append_events_size_bound(events.clone())
                .uncompressed_len()?;
        Ok(Self {
            events,
            encoded_bytes,
        })
    }
}

impl service_executor::GroupCommitRequest for IngestBatchRequest {
    type Item = EventEnvelope;
    type Key = SignalKind;

    fn key(&self) -> Self::Key {
        self.events[0].signal
    }

    fn item_count(&self) -> usize {
        self.events.len()
    }

    fn encoded_bytes(&self) -> usize {
        self.encoded_bytes
    }

    fn into_items(self) -> Vec<Self::Item> {
        self.events
    }
}

#[derive(Default)]
pub(crate) struct IngestBatchCoordinator {
    pub(in crate::ingest) queue:
        Option<service_executor::GroupCommitQueue<IngestBatchRequest, AppendResult, anyhow::Error>>,
    pub(in crate::ingest) worker: Option<service_executor::GroupCommitWorker>,
}
