//! Ingest use cases own the injected journal, queue and capacity interfaces.

use crate::ingest::domain::batch_submission::BatchSubmission;
use crate::ingest::domain::capacity_port::IngestCapacity;
use crate::journal::application::batch_limits::JournalBatchSizing;
use crate::journal::application::commit_append_batch::CommitContext;
use crate::journal::application::journal_service::DurableJournal;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub(crate) struct IngestService {
    pub(super) journal: Arc<DurableJournal>,
    pub(super) draining: Arc<AtomicBool>,
    pub(super) context: CommitContext,
    pub(super) journal_batches: Arc<JournalBatchSizing>,
    pub(super) batch_submission: Arc<dyn BatchSubmission<Error = anyhow::Error>>,
    pub(super) capacity: Arc<dyn IngestCapacity<Error = storage_durable::CapacityError>>,
}

impl IngestService {
    pub(crate) fn new(
        journal: Arc<DurableJournal>,
        draining: Arc<AtomicBool>,
        context: CommitContext,
        journal_batches: Arc<JournalBatchSizing>,
        batch_submission: Arc<dyn BatchSubmission<Error = anyhow::Error>>,
        capacity: Arc<dyn IngestCapacity<Error = storage_durable::CapacityError>>,
    ) -> Self {
        Self {
            journal,
            draining,
            context,
            journal_batches,
            batch_submission,
            capacity,
        }
    }
}
