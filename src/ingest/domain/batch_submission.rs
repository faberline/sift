//! A session submits already governed batches in their original order.

use crate::shared_kernel::append_result::AppendResult;
use crate::shared_kernel::event::EventEnvelope;
use crate::shared_kernel::port_future::PortFuture;

pub(crate) trait BatchSession: Send + Sync {
    type Error;
    fn submit(&self, events: Vec<EventEnvelope>) -> PortFuture<'_, Vec<AppendResult>, Self::Error>;
}

pub(crate) trait BatchSubmission: Send + Sync {
    type Error;
    fn begin(&self) -> Result<Box<dyn BatchSession<Error = Self::Error>>, Self::Error>;
    fn close(&self);
    fn join(&self) -> PortFuture<'_, (), Self::Error>;
}
