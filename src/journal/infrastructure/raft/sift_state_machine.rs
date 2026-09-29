//! The Sift state machine: applies committed Raft batches to the canonical
//! per-signal WAL, and holds the retention fence and checkpoint positions.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use raft_runtime::{OutcomeWindow, RaftStateMachine};

use crate::journal::domain::append_result::AppendResult;
use crate::journal::domain::checkpoint_position::CheckpointPosition;
use crate::journal::domain::control_state::{ControlState, CONTROL_STATE_FORMAT_VERSION};
use crate::journal::domain::retention_fence::RetentionFenceV1;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::journal::infrastructure::raft::archive_checkpoint::ArchiveCheckpointV1;
use crate::journal::infrastructure::raft::archive_checkpoint_install::ValidatedArchiveStage;
use crate::journal::infrastructure::raft::control_state_store::{
    persist_control, CONTROL_STATE_FILE,
};
use crate::journal::infrastructure::raft::local_checkpoint::LocalCheckpointV1;
use crate::journal::infrastructure::raft::resident_checkpoint::ResidentCheckpointV1;
use crate::storage::SegmentManifest;
use crate::SignalKind;

#[cfg(any(not(unix), unix))]
use crate::journal::infrastructure::raft::control_state_store::set_directory_mode;

const APPEND_OUTCOME_WINDOW: u64 = 64;

/// Applies one committed Raft batch to the canonical per-signal WAL.
pub struct SiftStateMachine {
    pub(super) journal: Arc<DurableJournal>,
    pub(super) commit_gate: Mutex<()>,
    pub(super) control_path: PathBuf,
    pub(super) control: Mutex<ControlState>,
    pub(super) append_outcomes: Mutex<OutcomeWindow<Vec<AppendResult>>>,
    pub(super) checkpoint_position: Mutex<CheckpointPosition>,
    pub(super) archive_checkpoint: Mutex<Option<ArchiveCheckpointV1>>,
    pub(super) local_checkpoint: Mutex<Option<LocalCheckpointV1>>,
    pub(super) resident_checkpoint: Mutex<Option<ResidentCheckpointV1>>,
    pub(super) validated_archive_stage: Mutex<Option<ValidatedArchiveStage>>,
    pub(super) applied_index: AtomicU64,
}

impl SiftStateMachine {
    pub fn new(journal: Arc<DurableJournal>) -> Self {
        let data_dir = journal.data_dir().to_path_buf();
        Self::open(data_dir, journal).expect("open Sift state-machine control state")
    }

    pub fn open(data_dir: impl AsRef<Path>, journal: Arc<DurableJournal>) -> Result<Self> {
        let control_dir = data_dir.as_ref().join("control");
        fs::create_dir_all(&control_dir)
            .with_context(|| format!("create Sift control directory {}", control_dir.display()))?;
        set_directory_mode(&control_dir)?;
        let control_path = control_dir.join(CONTROL_STATE_FILE);
        let control =
            if control_path.exists() {
                serde_json::from_slice::<ControlState>(&fs::read(&control_path).with_context(
                    || format!("read Sift control state {}", control_path.display()),
                )?)
                .with_context(|| format!("decode Sift control state {}", control_path.display()))?
            } else {
                ControlState::default()
            };
        if control.format_version != CONTROL_STATE_FORMAT_VERSION {
            bail!(
                "unsupported Sift control state format {}",
                control.format_version
            );
        }
        journal.set_retention_fenced(control.pending_retention.is_some());
        persist_control(&control_path, &control)?;
        let applied_index = control.applied_index;
        let checkpoint_position = CheckpointPosition {
            applied_index,
            raw_cursor: journal.last_cursor(),
        };
        Ok(Self {
            journal,
            commit_gate: Mutex::new(()),
            control_path,
            control: Mutex::new(control),
            append_outcomes: Mutex::new(OutcomeWindow::new(APPEND_OUTCOME_WINDOW)),
            checkpoint_position: Mutex::new(checkpoint_position),
            archive_checkpoint: Mutex::new(None),
            local_checkpoint: Mutex::new(None),
            resident_checkpoint: Mutex::new(None),
            validated_archive_stage: Mutex::new(None),
            applied_index: AtomicU64::new(applied_index),
        })
    }

    pub fn applied_commit_index(&self) -> u64 {
        self.applied_index.load(Ordering::Acquire)
    }

    pub(crate) fn pending_retention_fence(&self) -> Option<(RetentionFenceV1, u64)> {
        let control = self
            .control
            .lock()
            .expect("Sift control state lock poisoned");
        control
            .pending_retention
            .clone()
            .map(|fence| (fence, control.applied_index))
    }

    pub(crate) fn clear_retention_fence_after_checkpoint(
        &self,
        retention_generation: u64,
    ) -> Result<()> {
        if crate::storage::archive::committed_status(self.journal.data_dir())?
            .is_some_and(|status| status.retention_scan_pending)
        {
            return Ok(());
        }
        let _gate = self
            .commit_gate
            .lock()
            .expect("Sift commit gate lock poisoned");
        let mut control = self
            .control
            .lock()
            .expect("Sift control state lock poisoned");
        if control
            .pending_retention
            .as_ref()
            .is_some_and(|fence| fence.target_generation <= retention_generation)
        {
            control.pending_retention = None;
            persist_control(&self.control_path, &control)?;
            self.journal.set_retention_fenced(false);
        }
        Ok(())
    }

    #[doc(hidden)]
    pub fn clear_retention_fence_after_checkpoint_for_diagnostics(
        &self,
        retention_generation: u64,
    ) -> Result<()> {
        self.clear_retention_fence_after_checkpoint(retention_generation)
    }

    #[doc(hidden)]
    pub fn retention_fence_pending_for_diagnostics(&self) -> bool {
        self.pending_retention_fence().is_some()
    }

    pub fn apply_local(&self, index: u64, command: &[u8]) -> Result<()> {
        <Self as RaftStateMachine>::apply(self, index, command)
    }

    pub fn take_append_outcomes(&self, index: u64) -> Option<Vec<AppendResult>> {
        self.append_outcomes
            .lock()
            .expect("Sift append outcome lock poisoned")
            .claim(index)
    }

    pub(crate) fn checkpoint_position(&self) -> (u64, u64) {
        let position = *self
            .checkpoint_position
            .lock()
            .expect("Sift checkpoint position lock poisoned");
        (position.applied_index, position.raw_cursor)
    }

    /// Seal one journal prefix while holding the same gate as Raft apply.
    ///
    /// The returned Raft index and raw cursor describe the same durable prefix.
    /// Upload can continue after this method returns while newer Raft entries
    /// append to the journal.
    #[doc(hidden)]
    pub fn capture_archive_prefix(&self) -> Result<(u64, u64, Vec<(SignalKind, SegmentManifest)>)> {
        let _gate = self
            .commit_gate
            .lock()
            .expect("Sift commit gate lock poisoned");
        let (raw_cursor, segments) = self.journal.seal_archive_prefix()?;
        let position = *self
            .checkpoint_position
            .lock()
            .expect("Sift checkpoint position lock poisoned");
        if raw_cursor != position.raw_cursor {
            bail!(
                "Sift archive cursor {raw_cursor} does not match Raft prefix cursor {}",
                position.raw_cursor
            );
        }
        Ok((position.applied_index, raw_cursor, segments))
    }

    /// Rewrite the committed archive prefix while newer Raft entries remain a
    /// local suffix. The archive code applies the retained prefix under the
    /// journal lock and preserves that suffix.
    pub(crate) fn expire_current_archive_at(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<crate::storage::archive::ExpirationReceipt>> {
        let Some(remote) = crate::storage::archive::remote_retained_state(self.journal.data_dir())?
        else {
            return Ok(None);
        };
        if self.journal.last_cursor() < remote.snapshot_index {
            bail!("Sift journal is behind its committed archive cursor");
        }
        let suffix_events = self.journal.last_cursor() - remote.snapshot_index;
        let expected_local_events = remote
            .event_count
            .checked_add(suffix_events)
            .context("Sift retained event count exhausted u64")?;
        let recovery_pending = self.journal.recovery_required()
            || self.journal.total_event_count() != expected_local_events
            || self.journal.retention_generation() != remote.retention_generation;
        let receipt = crate::storage::archive::expire_committed_events_at(&self.journal, now)?;
        if receipt.expired_events > 0 || recovery_pending {
            Ok(Some(receipt))
        } else {
            Ok(None)
        }
    }
}
