//! Raft coordination with pure command data and boxed futures.

use crate::shared_kernel::checkpoint_data::SnapshotCompactionOutcome;
use crate::shared_kernel::port_future::{PortFuture, PortValueFuture};
use crate::shared_kernel::sift_command::SiftCommandV1;

pub(crate) trait RaftCheckpointPort: Send + Sync {
    type Error;
    fn encode(&self, command: &SiftCommandV1) -> Result<Vec<u8>, Self::Error>;
    fn propose(&self, command: Vec<u8>) -> PortFuture<'_, u64, Self::Error>;
    fn is_leader(&self) -> PortValueFuture<'_, bool>;
    fn require_snapshot_capability_on_all_voters(&self) -> PortFuture<'_, (), Self::Error>;
    fn require_applied_index_on_all_voters(
        &self,
        applied_index: u64,
    ) -> PortFuture<'_, (), Self::Error>;
    fn snapshot_and_compact_through_outcome(
        &self,
        up_to: u64,
    ) -> PortFuture<'_, SnapshotCompactionOutcome, Self::Error>;
    fn snapshot_and_compact_through_quorum_outcome(
        &self,
        up_to: u64,
    ) -> PortFuture<'_, SnapshotCompactionOutcome, Self::Error>;
}
