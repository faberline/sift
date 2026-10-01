//! Assemble checkpoint services over the original state machine and Raft host.

use crate::app::service_state::ServiceState;
use crate::archive::application::lifecycle_capacity::LifecycleCapacityService;
use crate::archive::application::run_lifecycle_attempt::LifecycleContext;
use crate::archive::infrastructure::lifecycle_capacity_adapter::NativeLifecycleCapacity;
use crate::journal::application::checkpoint_service::JournalCheckpointService;
use crate::journal::application::raft_checkpoint_service::RaftCheckpointService;
use crate::journal::infrastructure::raft::raft_checkpoint_adapter::NativeRaftCheckpoint;
use crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine;
use std::sync::Arc;

type CheckpointServices = (
    Arc<JournalCheckpointService>,
    Option<Arc<RaftCheckpointService>>,
);

pub(super) fn checkpoint_services(
    state_machine: Arc<SiftStateMachine>,
    host: Option<&Arc<raft_runtime::RaftHost>>,
) -> CheckpointServices {
    (
        Arc::new(JournalCheckpointService::new(state_machine)),
        host.map(|host| {
            Arc::new(RaftCheckpointService::new(Arc::new(NativeRaftCheckpoint {
                host: host.clone(),
            })))
        }),
    )
}

impl ServiceState {
    pub(crate) fn lifecycle_context(&self, destination: Option<String>) -> LifecycleContext {
        LifecycleContext {
            services: self.archive_lifecycle.services().clone(),
            journal: self.journal.clone(),
            raft: self.checkpoint_raft.clone(),
            state_machine: self.checkpoints.clone(),
            destination,
            local_capacity: Arc::new(LifecycleCapacityService::new(Arc::new(
                NativeLifecycleCapacity {
                    capacity: self.local_capacity.clone(),
                },
            ))),
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support;
