//! The durable journal over raw storage: its state, whether it is recovered and
//! queryable, how it is opened, and its accessors.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

use anyhow::{bail, Result};
use metrics_prometheus::Counter;

use crate::event::EventEnvelope;
use crate::ingest::domain::governance_policy::GovernancePolicySet;
use crate::journal::domain::journal_limits::DEFAULT_RESIDENT_JOURNAL_EVENTS;
use crate::journal::domain::journal_state::JournalState;
use crate::node::domain::storage_role::StorageRole;
use crate::node::infrastructure::data_layout::DataLayout;

/// Append-only JSONL journal. State is updated only after `sync_data` succeeds,
/// making a successful [`append`](Self::append) acknowledgement durable.
pub struct DurableJournal {
    pub(in crate::journal) _layout: DataLayout,
    pub(in crate::journal) wal: crate::storage::SignalWal,
    pub(in crate::journal) storage:
        crate::journal::infrastructure::storage::raw_storage::RawStorage,
    pub(in crate::journal) dedupe: crate::storage::DedupeIndex,
    pub(in crate::journal) blob_gate: Mutex<()>,
    pub(in crate::journal) state: RwLock<JournalState>,
    pub(in crate::journal) resident_limit: usize,
    pub(in crate::journal) governance: GovernancePolicySet,
    pub(in crate::journal) accepted: Counter,
    pub(in crate::journal) duplicates: Counter,
    pub(in crate::journal) fsyncs: Counter,
    pub(in crate::journal) recovery_required: AtomicBool,
    pub(in crate::journal) retention_fenced: AtomicBool,
}

impl DurableJournal {
    pub(in crate::journal) fn ensure_recovered(&self) -> Result<()> {
        if self.recovery_required.load(Ordering::Acquire) {
            bail!("Sift journal requires archive recovery before it can serve data");
        }
        Ok(())
    }

    pub(crate) fn ensure_queryable(&self) -> Result<()> {
        self.ensure_recovered()?;
        if self.retention_fenced.load(Ordering::Acquire) {
            bail!("Sift queries wait for the committed retention checkpoint");
        }
        Ok(())
    }

    pub(crate) fn set_retention_fenced(&self, fenced: bool) {
        self.retention_fenced.store(fenced, Ordering::Release);
    }

    pub(crate) fn mark_recovery_required(&self) {
        self.recovery_required.store(true, Ordering::Release);
    }

    pub(crate) fn recovery_required(&self) -> bool {
        self.recovery_required.load(Ordering::Acquire)
    }

    pub(crate) fn retention_generation(&self) -> u64 {
        self.state
            .read()
            .expect("journal state lock poisoned")
            .retention_generation
    }

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
        Self::open_configured(data_dir, governance, role, DEFAULT_RESIDENT_JOURNAL_EVENTS)
    }

    pub fn open_with_resident_limit(
        data_dir: impl AsRef<Path>,
        resident_limit: usize,
    ) -> Result<Self> {
        Self::open_configured(
            data_dir,
            GovernancePolicySet::from_env()?,
            StorageRole::All,
            resident_limit,
        )
    }

    pub fn govern_event(&self, event: EventEnvelope) -> Result<EventEnvelope> {
        self.governance.govern(event)
    }

    pub fn storage(&self) -> &crate::journal::infrastructure::storage::raw_storage::RawStorage {
        &self.storage
    }

    pub(crate) fn last_cursor(&self) -> u64 {
        self.state
            .read()
            .expect("journal state lock poisoned")
            .last_cursor
    }

    pub(crate) fn projection_generation(&self) -> u64 {
        self.state
            .read()
            .expect("journal state lock poisoned")
            .projection_generation
    }

    pub(crate) fn data_dir(&self) -> &Path {
        self._layout.root()
    }

    pub(crate) fn snapshot_bounds(&self) -> (u64, u64) {
        let state = self.state.read().expect("journal state lock poisoned");
        (state.last_cursor, state.total_events)
    }

    pub fn resident_event_count(&self) -> usize {
        self.state
            .read()
            .expect("journal state lock poisoned")
            .recent_events
            .len()
    }

    pub fn total_event_count(&self) -> u64 {
        self.state
            .read()
            .expect("journal state lock poisoned")
            .total_events
    }
}
