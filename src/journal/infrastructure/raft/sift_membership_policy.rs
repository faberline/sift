//! Sift's replicated topology: exactly three durable voters per shard.

use anyhow::bail;

pub(crate) struct SiftMembershipPolicy;

impl raft_runtime::MembershipPolicy for SiftMembershipPolicy {
    fn validate(&self, topology: &raft_runtime::ClusterTopology) -> anyhow::Result<()> {
        if topology.replicas_per_shard != 3
            || topology.membership.voters.len() != 3
            || !topology.membership.learners.is_empty()
        {
            bail!("Sift replicated mode requires exactly three durable voting replicas per shard");
        }
        Ok(())
    }
}
