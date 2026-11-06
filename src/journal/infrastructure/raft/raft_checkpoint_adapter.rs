//! Coordinate checkpoints through the original shared Raft host.

use crate::journal::domain::raft_checkpoint_port::RaftCheckpointPort;
use crate::shared_kernel::checkpoint_data::SnapshotCompactionOutcome;
use crate::shared_kernel::port_future::{PortFuture, PortValueFuture};
use crate::shared_kernel::sift_command::SiftCommandV1;
use std::sync::Arc;

pub(crate) struct NativeRaftCheckpoint {
    pub(crate) host: Arc<raft_runtime::RaftHost>,
}

impl RaftCheckpointPort for NativeRaftCheckpoint {
    type Error = anyhow::Error;
    fn encode(&self, command: &SiftCommandV1) -> Result<Vec<u8>, Self::Error> {
        command.encoded()
    }
    fn propose(&self, command: Vec<u8>) -> PortFuture<'_, u64, Self::Error> {
        Box::pin(self.host.propose(command))
    }
    fn is_leader(&self) -> PortValueFuture<'_, bool> {
        Box::pin(self.host.is_leader())
    }
    fn require_snapshot_capability_on_all_voters(&self) -> PortFuture<'_, (), Self::Error> {
        Box::pin(self.host.require_snapshot_capability_on_all_voters())
    }
    fn require_applied_index_on_all_voters(
        &self,
        applied_index: u64,
    ) -> PortFuture<'_, (), Self::Error> {
        Box::pin(self.host.require_applied_index_on_all_voters(applied_index))
    }
    fn snapshot_and_compact_through_outcome(
        &self,
        up_to: u64,
    ) -> PortFuture<'_, SnapshotCompactionOutcome, Self::Error> {
        Box::pin(async move {
            let outcome = self
                .host
                .snapshot_and_compact_through_outcome(up_to)
                .await?;
            Ok(SnapshotCompactionOutcome {
                snapshot_index: outcome.snapshot_index,
                installed: outcome.installed,
            })
        })
    }
    fn snapshot_and_compact_through_quorum_outcome(
        &self,
        up_to: u64,
    ) -> PortFuture<'_, SnapshotCompactionOutcome, Self::Error> {
        Box::pin(async move {
            let outcome = self
                .host
                .snapshot_and_compact_through_quorum_outcome(up_to)
                .await?;
            Ok(SnapshotCompactionOutcome {
                snapshot_index: outcome.snapshot_index,
                installed: outcome.installed,
            })
        })
    }
}
