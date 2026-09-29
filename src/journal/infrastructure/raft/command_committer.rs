//! Committing a replicated command through Raft, or locally without it.

use anyhow::Result;

use crate::journal::application::commit_append_batch::CommitContext;

impl CommitContext {
    pub(in crate::journal) async fn commit_command(
        &self,
        command: crate::journal::domain::sift_command::SiftCommandV1,
    ) -> Result<u64> {
        let bytes = command.encoded()?;
        if let Some(raft) = &self.raft {
            return raft.propose(bytes).await;
        }
        let _guard = self.local_command.lock().await;
        let index = self.state_machine.applied_commit_index() + 1;
        self.state_machine.apply_local(index, &bytes)?;
        Ok(index)
    }
}
