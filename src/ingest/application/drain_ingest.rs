//! Draining ingest: refusing new batches and waiting for accepted ones.

use std::sync::atomic::Ordering;

use anyhow::Result;

use crate::app::service_state::ServiceState;

impl ServiceState {
    pub fn start_drain(&self) {
        self.draining.store(true, Ordering::Release);
        self.batch_submission.close();
    }

    /// Stop accepting new batches and wait until every accepted batch has a
    /// durable result. This also releases the journal lock held by the worker.
    pub async fn finish_drain(&self) -> Result<()> {
        self.start_drain();
        self.batch_submission.join().await?;
        Ok(())
    }

    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::Acquire)
    }
}
