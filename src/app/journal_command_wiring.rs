//! Assembly of command, capacity and governance ports over shared backends.

use crate::app::service_state::ServiceState;
use crate::ingest::domain::governance_policy::GovernancePolicySet;
use crate::journal::application::commit_append_batch::CommitContext;
use crate::journal::domain::command_submission::{
    CapacityReservation, JournalCapacity, JournalGovernance, JournalReceiptLookup,
};
use crate::journal::domain::journal_limits::DEFAULT_RESIDENT_JOURNAL_EVENTS;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::journal::infrastructure::raft::command_committer::RaftCommandCommitter;
use crate::node::domain::storage_role::StorageRole;
use crate::node::infrastructure::local_capacity::LocalCapacity;
use crate::shared_kernel::append_result::AppendResult;
use crate::shared_kernel::event::EventEnvelope;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Arc;

struct GovernanceAdapter(GovernancePolicySet);
impl JournalGovernance for GovernanceAdapter {
    type Error = anyhow::Error;
    fn validate(&self) -> Result<()> {
        Ok(self.0.validate()?)
    }
    fn govern(&self, event: EventEnvelope) -> Result<EventEnvelope> {
        Ok(self.0.govern(event)?)
    }
}

struct ReceiptAdapter(Arc<DurableJournal>);
impl JournalReceiptLookup for ReceiptAdapter {
    type Error = anyhow::Error;
    fn result_for_at(
        &self,
        project: &str,
        event_id: &str,
        at: DateTime<Utc>,
    ) -> Result<Option<AppendResult>> {
        self.0.result_for_at(project, event_id, at)
    }
}

struct CapacityAdapter(Arc<LocalCapacity>);
impl JournalCapacity for CapacityAdapter {
    type Error = anyhow::Error;
    fn reserve(&self, bytes: u64) -> Result<Box<dyn CapacityReservation + '_>> {
        Ok(Box::new(self.0.reserve(bytes)?))
    }
}
impl CapacityReservation for storage_durable::CapacityReservation<'_> {
    fn commit(self: Box<Self>) {
        (*self).commit();
    }
}

impl ServiceState {
    pub(crate) fn commit_context(&self) -> CommitContext {
        commit_context(
            self.journal.clone(),
            self.raft.clone(),
            self.state_machine.clone(),
            self.local_command.clone(),
            self.local_capacity.clone(),
        )
    }
}

impl DurableJournal {
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_role(data_dir, StorageRole::All)
    }
    pub fn open_with_role(data_dir: impl AsRef<Path>, role: StorageRole) -> Result<Self> {
        Self::open_with_governance_and_role(data_dir, GovernancePolicySet::from_env()?, role)
    }
    pub fn open_with_governance(
        data_dir: impl AsRef<Path>,
        governance: GovernancePolicySet,
    ) -> Result<Self> {
        Self::open_with_governance_and_role(data_dir, governance, StorageRole::All)
    }
    pub fn open_with_governance_and_role(
        data_dir: impl AsRef<Path>,
        governance: GovernancePolicySet,
        role: StorageRole,
    ) -> Result<Self> {
        Self::open_configured(
            data_dir,
            Arc::new(GovernanceAdapter(governance)),
            role,
            DEFAULT_RESIDENT_JOURNAL_EVENTS,
        )
    }
    pub fn open_with_resident_limit(
        data_dir: impl AsRef<Path>,
        resident_limit: usize,
    ) -> Result<Self> {
        Self::open_configured(
            data_dir,
            Arc::new(GovernanceAdapter(GovernancePolicySet::from_env()?)),
            StorageRole::All,
            resident_limit,
        )
    }
}

pub(super) fn commit_context(
    journal: Arc<DurableJournal>,
    raft: Option<Arc<raft_runtime::RaftHost>>,
    state_machine: Arc<crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine>,
    local_command: Arc<tokio::sync::Mutex<()>>,
    local_capacity: Arc<LocalCapacity>,
) -> CommitContext {
    CommitContext {
        journal: Arc::new(ReceiptAdapter(journal)),
        commands: Arc::new(RaftCommandCommitter {
            raft,
            state_machine,
            local_command,
        }),
        local_capacity: Arc::new(CapacityAdapter(local_capacity)),
    }
}
