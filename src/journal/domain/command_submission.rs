//! The durable command boundary, independent of Raft and its executor.

use crate::shared_kernel::append_result::AppendResult;
use crate::shared_kernel::event::EventEnvelope;
use crate::shared_kernel::port_future::PortFuture;
use crate::shared_kernel::sift_command::SiftCommandV1;
use chrono::{DateTime, Utc};

pub(crate) trait CommandSubmission: Send + Sync {
    type Error;
    fn applied_commit_index(&self) -> u64;
    fn take_append_outcomes(&self, index: u64) -> Option<Vec<AppendResult>>;
    fn append_size_bound(&self, events: Vec<EventEnvelope>) -> Result<usize, Self::Error>;
    fn commit_command(&self, command: SiftCommandV1) -> PortFuture<'_, u64, Self::Error>;
}

pub(crate) trait JournalReceiptLookup: Send + Sync {
    type Error;
    fn result_for_at(
        &self,
        project: &str,
        event_id: &str,
        at: DateTime<Utc>,
    ) -> Result<Option<AppendResult>, Self::Error>;
}

pub(crate) trait JournalGovernance: Send + Sync {
    type Error;
    fn validate(&self) -> Result<(), Self::Error>;
    fn govern(&self, event: EventEnvelope) -> Result<EventEnvelope, Self::Error>;
}

pub(crate) trait CapacityReservation: Send {
    fn commit(self: Box<Self>);
}

pub(crate) trait JournalCapacity: Send + Sync {
    type Error;
    fn reserve(&self, bytes: u64) -> Result<Box<dyn CapacityReservation + '_>, Self::Error>;
}
