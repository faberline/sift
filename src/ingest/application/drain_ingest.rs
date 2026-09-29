//! Draining ingest: refusing new batches and waiting for accepted ones.

use std::sync::atomic::Ordering;

use anyhow::{Context, Result};

use crate::ServiceState;

impl ServiceState {
    pub fn start_drain(&self) {
        self.draining.store(true, Ordering::Release);
        self.batch_coordinator
            .lock()
            .expect("Sift ingest batch coordinator lock poisoned")
            .queue
            .take();
    }

    /// Stop accepting new batches and wait until every accepted batch has a
    /// durable result. This also releases the journal lock held by the worker.
    pub async fn finish_drain(&self) -> Result<()> {
        self.start_drain();
        let worker = self
            .batch_coordinator
            .lock()
            .expect("Sift ingest batch coordinator lock poisoned")
            .worker
            .take();
        if let Some(worker) = worker {
            worker
                .join()
                .await
                .context("join Sift ingest batch coordinator")?;
        }
        Ok(())
    }

    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::Acquire)
    }
}
