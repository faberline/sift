//! The group-commit batch the journal commits, and the queue and worker that
//! collect ingest requests into it.

use anyhow::Result;

use crate::shared_kernel::append_result::AppendResult;
use crate::shared_kernel::event::{EventEnvelope, SignalKind};

pub(in crate::ingest) struct IngestBatchRequest {
    events: Vec<EventEnvelope>,
    encoded_bytes: usize,
}

impl IngestBatchRequest {
    pub(in crate::ingest) fn new(events: Vec<EventEnvelope>, encoded_bytes: usize) -> Self {
        Self {
            events,
            encoded_bytes,
        }
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

pub(crate) struct IngestBatchBackend {
    pub(crate) coordinator: std::sync::Mutex<IngestBatchCoordinator>,
    pub(crate) draining: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) sizing:
        std::sync::Arc<crate::journal::application::batch_limits::JournalBatchSizing>,
    pub(crate) context: crate::journal::application::commit_append_batch::CommitContext,
}

struct QueueSession {
    queue: service_executor::GroupCommitQueue<IngestBatchRequest, AppendResult, anyhow::Error>,
    sizing: std::sync::Arc<crate::journal::application::batch_limits::JournalBatchSizing>,
}

impl crate::ingest::domain::batch_submission::BatchSession for QueueSession {
    type Error = anyhow::Error;
    fn submit(
        &self,
        events: Vec<EventEnvelope>,
    ) -> crate::shared_kernel::port_future::PortFuture<'_, Vec<AppendResult>, Self::Error> {
        Box::pin(async move {
            let encoded_bytes = self.sizing.encoded_size_bound(events.clone())?;
            let request = IngestBatchRequest::new(events, encoded_bytes);
            self.queue
                .submit(request)
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))
        })
    }
}

impl crate::ingest::domain::batch_submission::BatchSubmission for IngestBatchBackend {
    type Error = anyhow::Error;
    fn begin(
        &self,
    ) -> Result<Box<dyn crate::ingest::domain::batch_submission::BatchSession<Error = Self::Error>>>
    {
        let mut coordinator = self
            .coordinator
            .lock()
            .expect("Sift ingest batch coordinator lock poisoned");
        if self.draining.load(std::sync::atomic::Ordering::Acquire) {
            anyhow::bail!("Sift is draining and cannot start an ingest batch coordinator");
        }
        if let Some(queue) = coordinator.queue.as_ref() {
            return Ok(Box::new(QueueSession {
                queue: queue.clone(),
                sizing: self.sizing.clone(),
            }));
        }
        let config = service_executor::GroupCommitConfig::new(
            crate::journal::application::batch_limits::RAFT_BATCH_MAX_DELAY,
            crate::journal::application::batch_limits::RAFT_BATCH_MAX_ITEMS,
            crate::journal::application::batch_limits::RAFT_BATCH_MAX_BYTES,
        )?;
        let context = self.context.clone();
        let (queue, worker) =
            service_executor::spawn_group_commit(config, move |events: Vec<EventEnvelope>| {
                let context = context.clone();
                async move { context.append_governed_batch(events).await }
            });
        coordinator.queue = Some(queue.clone());
        coordinator.worker = Some(worker);
        Ok(Box::new(QueueSession {
            queue,
            sizing: self.sizing.clone(),
        }))
    }
    fn close(&self) {
        self.coordinator
            .lock()
            .expect("Sift ingest batch coordinator lock poisoned")
            .queue
            .take();
    }
    fn join(&self) -> crate::shared_kernel::port_future::PortFuture<'_, (), Self::Error> {
        Box::pin(async move {
            let worker = self
                .coordinator
                .lock()
                .expect("Sift ingest batch coordinator lock poisoned")
                .worker
                .take();
            if let Some(worker) = worker {
                use anyhow::Context;
                worker
                    .join()
                    .await
                    .context("join Sift ingest batch coordinator")?;
            }
            Ok(())
        })
    }
}
