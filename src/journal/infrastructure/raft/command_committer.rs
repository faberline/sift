//! The Raft and single-node implementations of the command submission port.

use crate::journal::domain::command_submission::CommandSubmission;
use crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine;
use crate::shared_kernel::append_result::AppendResult;
use crate::shared_kernel::event::EventEnvelope;
use crate::shared_kernel::port_future::PortFuture;
use crate::shared_kernel::sift_command::SiftCommandV1;
use std::sync::Arc;

pub(crate) struct RaftCommandCommitter {
    pub(crate) raft: Option<Arc<raft_runtime::RaftHost>>,
    pub(crate) state_machine: Arc<SiftStateMachine>,
    pub(crate) local_command: Arc<tokio::sync::Mutex<()>>,
}

impl CommandSubmission for RaftCommandCommitter {
    type Error = anyhow::Error;
    fn applied_commit_index(&self) -> u64 {
        self.state_machine.applied_commit_index()
    }
    fn take_append_outcomes(&self, index: u64) -> Option<Vec<AppendResult>> {
        self.state_machine.take_append_outcomes(index)
    }
    fn append_size_bound(&self, events: Vec<EventEnvelope>) -> Result<usize, Self::Error> {
        SiftCommandV1::append_events_size_bound(events).uncompressed_len()
    }
    fn commit_command(&self, command: SiftCommandV1) -> PortFuture<'_, u64, Self::Error> {
        Box::pin(async move {
            let bytes = command.encoded()?;
            if let Some(raft) = &self.raft {
                return raft.propose(bytes).await;
            }
            let _guard = self.local_command.lock().await;
            let index = self.state_machine.applied_commit_index() + 1;
            self.state_machine.apply_local(index, &bytes)?;
            Ok(index)
        })
    }
}
