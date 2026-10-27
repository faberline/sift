//! Journal batch limits and the command codec's exact wire size bound.

use crate::journal::domain::command_submission::CommandSubmission;
use crate::shared_kernel::event::EventEnvelope;
use anyhow::Result;
use std::sync::Arc;

pub(crate) use crate::shared_kernel::raft_batch_limits::{
    RAFT_BATCH_MAX_BYTES, RAFT_BATCH_MAX_DELAY, RAFT_BATCH_MAX_ITEMS,
};

pub(crate) struct JournalBatchSizing {
    commands: Arc<dyn CommandSubmission<Error = anyhow::Error>>,
}
impl JournalBatchSizing {
    pub(crate) fn new(commands: Arc<dyn CommandSubmission<Error = anyhow::Error>>) -> Self {
        Self { commands }
    }
    pub(crate) fn encoded_size_bound(&self, events: Vec<EventEnvelope>) -> Result<usize> {
        self.commands.append_size_bound(events)
    }
}
