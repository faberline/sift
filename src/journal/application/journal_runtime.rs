//! The durable journal over raw storage: its state, whether it is recovered and
//! queryable, how it is opened, and its accessors.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{bail, Result};
use metrics_prometheus::Counter;

use crate::journal::domain::archive_access::{ArchiveMaintenance, ArchiveRead};
use crate::journal::domain::command_submission::JournalGovernance;
use crate::journal::domain::dedupe_store::DedupeStore;
use crate::journal::domain::event_store::RawEventStore;
use crate::journal::domain::recovery_store::{JournalRoot, RecoveryStore};
use crate::journal::domain::resident_window::ResidentWindow;
use crate::journal::domain::snapshot_ports::{JournalStaging, SnapshotCodec};
use crate::journal::domain::wal_store::WalStore;
use crate::shared_kernel::event::EventEnvelope;

/// Append-only JSONL journal. State is updated only after `sync_data` succeeds,
/// making a successful [`append`](Self::append) acknowledgement durable.
pub(crate) struct JournalRuntime {
    pub(super) _layout: Arc<dyn JournalRoot>,
    pub(super) wal: Arc<dyn WalStore<Error = anyhow::Error>>,
    pub(super) storage: Arc<dyn RawEventStore<Error = anyhow::Error>>,
    pub(super) dedupe: Arc<dyn DedupeStore<Error = anyhow::Error>>,
    pub(super) resources: Arc<dyn RecoveryStore<Error = anyhow::Error>>,
    pub(super) archive_read: Arc<dyn ArchiveRead<Error = anyhow::Error>>,
    pub(super) archive_maintenance: Arc<dyn ArchiveMaintenance<Error = anyhow::Error>>,
    pub(super) snapshots: Arc<dyn SnapshotCodec<Error = anyhow::Error>>,
    pub(super) staging: Arc<dyn JournalStaging<Error = anyhow::Error>>,
    pub(super) blob_gate: Mutex<()>,
    pub(super) state: RwLock<ResidentWindow>,
    pub(super) resident_limit: usize,
    pub(super) governance: Arc<dyn JournalGovernance<Error = anyhow::Error>>,
    pub(super) accepted: Counter,
    pub(super) duplicates: Counter,
    pub(super) fsyncs: Counter,
    pub(super) recovery_required: AtomicBool,
    pub(super) retention_fenced: AtomicBool,
}

impl JournalRuntime {
    pub(super) fn ensure_recovered(&self) -> Result<()> {
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

    pub fn govern_event(&self, event: EventEnvelope) -> Result<EventEnvelope> {
        self.governance.govern(event)
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
