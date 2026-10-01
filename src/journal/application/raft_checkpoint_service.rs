//! Lifecycle Raft use cases over an injected host port.

use crate::journal::domain::raft_checkpoint_port::RaftCheckpointPort;
use crate::shared_kernel::checkpoint_data::SnapshotCompactionOutcome;
use crate::shared_kernel::sift_command::SiftCommandV1;
use anyhow::Result;
use std::sync::Arc;

pub(crate) struct RaftCheckpointService {
    port: Arc<dyn RaftCheckpointPort<Error = anyhow::Error>>,
}

impl RaftCheckpointService {
    pub(crate) fn new(port: Arc<dyn RaftCheckpointPort<Error = anyhow::Error>>) -> Self {
        Self { port }
    }
    pub(crate) fn encode(&self, command: &SiftCommandV1) -> Result<Vec<u8>> {
        self.port.encode(command)
    }
    pub(crate) async fn propose(&self, command: Vec<u8>) -> Result<u64> {
        self.port.propose(command).await
    }
    pub(crate) async fn is_leader(&self) -> bool {
        self.port.is_leader().await
    }
    pub(crate) async fn require_snapshot_capability_on_all_voters(&self) -> Result<()> {
        self.port.require_snapshot_capability_on_all_voters().await
    }
    pub(crate) async fn require_applied_index_on_all_voters(
        &self,
        applied_index: u64,
    ) -> Result<()> {
        self.port
            .require_applied_index_on_all_voters(applied_index)
            .await
    }
    pub(crate) async fn snapshot_and_compact_through_outcome(
        &self,
        up_to: u64,
    ) -> Result<SnapshotCompactionOutcome> {
        self.port.snapshot_and_compact_through_outcome(up_to).await
    }
    pub(crate) async fn snapshot_and_compact_through_quorum_outcome(
        &self,
        up_to: u64,
    ) -> Result<SnapshotCompactionOutcome> {
        self.port
            .snapshot_and_compact_through_quorum_outcome(up_to)
            .await
    }
}
