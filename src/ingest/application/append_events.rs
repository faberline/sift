//! Appending decoded events: governing them before Raft, splitting them into
//! batches, and committing them through the group-commit queue.

use anyhow::{bail, Result};

use crate::app::service_state::ServiceState;
use crate::event::EventEnvelope;
use crate::ingest::domain::raft_batch_splitter::split_governed_batches;
use crate::ingest::infrastructure::ingest_batch_coordinator::IngestBatchRequest;
use crate::journal::application::commit_append_batch::CommitContext;
use crate::journal::domain::append_result::AppendResult;
use crate::shared_kernel::single_signal_batch::ensure_single_signal;

impl ServiceState {
    /// Govern and durably commit one Raft batch. Every returned event shares
    /// the same commit index, which is acknowledged only after local apply (or
    /// quorum apply in three-replica mode).
    pub async fn append_batch(&self, events: Vec<EventEnvelope>) -> Result<Vec<AppendResult>> {
        if events.is_empty() {
            bail!("Sift Raft batch must not be empty");
        }
        ensure_single_signal(&events)?;
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
        ensure_single_signal(&events)?;
        let governed = self.govern_events(events)?;
        let chunks = split_governed_batches(governed)?;
        let queue = self.ingest_batch_queue()?;
        let mut results = Vec::new();
        for events in chunks {
            let request = IngestBatchRequest::new(events)?;
            results.extend(
                queue
                    .submit(request)
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?,
            );
        }
        Ok(results)
    }

    fn ingest_batch_queue(
        &self,
    ) -> Result<service_executor::GroupCommitQueue<IngestBatchRequest, AppendResult, anyhow::Error>>
    {
        let mut coordinator = self
            .batch_coordinator
            .lock()
            .expect("Sift ingest batch coordinator lock poisoned");
        if self.is_draining() {
            bail!("Sift is draining and cannot start an ingest batch coordinator");
        }
        if let Some(queue) = coordinator.queue.as_ref() {
            return Ok(queue.clone());
        }
        let config = service_executor::GroupCommitConfig::new(
            crate::journal::domain::raft_batch_limits::RAFT_BATCH_MAX_DELAY,
            crate::journal::domain::raft_batch_limits::RAFT_BATCH_MAX_ITEMS,
            crate::journal::domain::raft_batch_limits::RAFT_BATCH_MAX_BYTES,
        )?;
        let context = self.commit_context();
        let (queue, worker) =
            service_executor::spawn_group_commit(config, move |events: Vec<EventEnvelope>| {
                let context = context.clone();
                async move { context.append_governed_batch(events).await }
            });
        coordinator.queue = Some(queue.clone());
        coordinator.worker = Some(worker);
        Ok(queue)
    }

    fn commit_context(&self) -> CommitContext {
        CommitContext {
            journal: self.journal.clone(),
            raft: self.raft.clone(),
            state_machine: self.state_machine.clone(),
            local_command: self.local_command.clone(),
            local_capacity: self.local_capacity.clone(),
        }
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
