//! Appending decoded events: governing them before Raft, splitting them into
//! batches, and committing them through the group-commit queue.

use anyhow::{bail, Result};

use crate::app::service_state::ServiceState;
use crate::ingest::application::raft_batch_splitter::split_governed_batches;
use crate::shared_kernel::append_result::AppendResult;
use crate::shared_kernel::event::EventEnvelope;
use crate::shared_kernel::single_signal_batch::ensure_single_signal;

impl ServiceState {
    /// Govern and durably commit one Raft batch. Every returned event shares
    /// the same commit index, which is acknowledged only after local apply (or
    /// quorum apply in three-replica mode).
    pub async fn append_batch(&self, events: Vec<EventEnvelope>) -> Result<Vec<AppendResult>> {
        if events.is_empty() {
            bail!("Sift Raft batch must not be empty");
        }
        ensure_single_signal("Sift batch", &events)?;
        // Govern before the Raft proposal so sensitive content never enters a
        // replicated log, even transiently. DurableJournal repeats the policy
        // idempotently at the raw boundary for direct/single-node callers.
        let governed = self.govern_events(events)?;
        self.append_governed_batch(governed).await
    }

    /// Split one decoded ingest request into Raft commands below the 1 MiB
    /// hard limit. The caller receives outcomes in the original event order.
    pub async fn append_events(&self, events: Vec<EventEnvelope>) -> Result<Vec<AppendResult>> {
        if events.is_empty() {
            return Ok(Vec::new());
        }
        if self.is_draining() {
            bail!("Sift is draining and cannot accept a new ingest batch");
        }
        ensure_single_signal("Sift batch", &events)?;
        let governed = self.govern_events(events)?;
        let empty_size = self.journal_batches.encoded_size_bound(Vec::new())?;
        let chunks = split_governed_batches(
            governed,
            empty_size,
            crate::journal::application::batch_limits::RAFT_BATCH_MAX_ITEMS,
            crate::journal::application::batch_limits::RAFT_BATCH_MAX_BYTES,
        )?;
        let queue = self.batch_submission.begin()?;
        let mut results = Vec::new();
        for events in chunks {
            results.extend(queue.submit(events).await?);
        }
        Ok(results)
    }

    fn govern_events(&self, events: Vec<EventEnvelope>) -> Result<Vec<EventEnvelope>> {
        let mut governed = Vec::with_capacity(events.len());
        for event in events {
            let event = self.journal.govern_event(event)?;
            event.validate()?;
            governed.push(event);
        }
        Ok(governed)
    }

    async fn append_governed_batch(
        &self,
        governed: Vec<EventEnvelope>,
    ) -> Result<Vec<AppendResult>> {
        self.commit_context().append_governed_batch(governed).await
    }
}
