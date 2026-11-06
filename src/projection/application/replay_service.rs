//! Cold projection use cases over a pure replay port.

use anyhow::Result;

use crate::projection::domain::replay_port::ReplayHandle;
use crate::shared_kernel::stored_event::StoredEvent;

pub(crate) struct ReplayService<Q, P> {
    port: ReplayHandle<Q, P, anyhow::Error>,
}

impl<Q, P> ReplayService<Q, P> {
    pub(crate) fn new(port: ReplayHandle<Q, P, anyhow::Error>) -> Self {
        Self { port }
    }

    pub(crate) fn apply_idempotent(&self, event: &StoredEvent) -> Result<()> {
        self.port.apply_idempotent(event)
    }

    pub(crate) fn query(&self, query: &Q) -> Result<P> {
        self.port.query(query)
    }
}
