//! The Sift state machine as a shared Raft state machine: apply, snapshot,
//! restore.

use std::io::Read;
use std::sync::atomic::Ordering;

use anyhow::{bail, Result};
use raft_runtime::{Index, RaftStateMachine};

use crate::journal::domain::append_policy::{
    append_decision_time, merge_pending_retention, validate_events, validate_retention_fence,
};
use crate::journal::domain::checkpoint_position::CheckpointPosition;
use crate::journal::domain::control_state::{ControlState, CONTROL_STATE_FORMAT_VERSION};
use crate::journal::domain::raft_batch_limits::RAFT_BATCH_MAX_BYTES;
use crate::journal::domain::sift_command::SiftCommandV1;
use crate::journal::infrastructure::raft::archive_checkpoint::write_archive_checkpoint;
use crate::journal::infrastructure::raft::command_codec::{
    decode_command, COMMAND_MAGIC, MAX_ENCODED_COMMAND_BYTES,
};
use crate::journal::infrastructure::raft::control_state_store::persist_control;
use crate::journal::infrastructure::raft::local_checkpoint::write_local_checkpoint;
use crate::journal::infrastructure::raft::resident_checkpoint::write_resident_checkpoint;
use crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine;
use crate::journal::infrastructure::raft::snapshot_restore::{
    restore_streamed_snapshot, validate_streamed_snapshot,
};
use crate::journal::infrastructure::raft::snapshot_writer::write_snapshot;

impl RaftStateMachine for SiftStateMachine {
    fn snapshot_capability(&self) -> Option<&'static str> {
        // v6 requires leader-selected acknowledgement time on every new
        // append command. One immutable candidate digest is deployed across
        // all voters before coordinated checkpoints are allowed.
        Some("sift-checkpoint-v7")
    }

    fn apply(&self, index: Index, command: &[u8]) -> Result<()> {
        if index <= self.applied_index.load(Ordering::Acquire) {
            return Ok(());
        }
        let command_limit = if command.starts_with(COMMAND_MAGIC) {
            MAX_ENCODED_COMMAND_BYTES
        } else {
            RAFT_BATCH_MAX_BYTES
        };
        if command.len() > command_limit {
            bail!("Sift Raft batch exceeds its wire limit");
        }
        let command = decode_command(command)?;
        let _gate = self
            .commit_gate
            .lock()
            .expect("Sift commit gate lock poisoned");
        if index <= self.applied_index.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut pending_retention_update = None;
        let results = match command {
            SiftCommandV1::AppendEvents {
                acknowledged_at,
                events,
            } => {
                validate_events(&events)?;
                let acknowledged_at = append_decision_time(acknowledged_at.as_deref(), &events)?;
                self.journal
                    .append_durable_batch_at(events, acknowledged_at)?
                    .into_iter()
                    .map(|result| result.with_commit_index(index))
                    .collect()
            }
            SiftCommandV1::ArchiveCheckpointBarrier {
                retention_generation,
                manifest_uri,
                manifest_sha256,
            } => {
                if !manifest_uri.starts_with("gs://")
                    || manifest_sha256.len() != 64
                    || !manifest_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    bail!("Sift archive checkpoint barrier identity is invalid");
                }
                if self.journal.retention_generation() > retention_generation {
                    bail!("Sift archive checkpoint barrier moved retention backwards");
                }
                Vec::new()
            }
            SiftCommandV1::RetentionFence { fence } => {
                validate_retention_fence(&fence)?;
                if fence.target_generation < self.journal.retention_generation() {
                    bail!("Sift retention fence moved retention backwards");
                }
                pending_retention_update = Some(
                    (self.journal.retention_generation() < fence.target_generation)
                        .then_some(fence),
                );
                Vec::new()
            }
            SiftCommandV1::ClearRetentionFence {
                retention_generation,
            } => {
                if retention_generation == 0
                    || self.journal.retention_generation() < retention_generation
                    || crate::storage::archive::committed_status(self.journal.data_dir())?
                        .is_some_and(|status| status.retention_scan_pending)
                {
                    bail!("Sift retention fence clear is not covered by committed retention");
                }
                let current = self
                    .control
                    .lock()
                    .expect("Sift control state lock poisoned")
                    .pending_retention
                    .clone();
                if current
                    .as_ref()
                    .is_some_and(|fence| fence.target_generation <= retention_generation)
                {
                    pending_retention_update = Some(None);
                }
                Vec::new()
            }
        };

        let mut control = self
            .control
            .lock()
            .expect("Sift control state lock poisoned");
        control.applied_index = index;
        if let Some(update) = pending_retention_update {
            control.pending_retention = update;
        }
        persist_control(&self.control_path, &control)?;
        self.journal
            .set_retention_fenced(control.pending_retention.is_some());
        *self
            .checkpoint_position
            .lock()
            .expect("Sift checkpoint position lock poisoned") = CheckpointPosition {
            applied_index: index,
            raw_cursor: self.journal.last_cursor(),
        };
        let mut outcomes = self
            .append_outcomes
            .lock()
            .expect("Sift append outcome lock poisoned");
        outcomes.insert(index, results);
        outcomes.advance(index);
        self.applied_index.store(index, Ordering::Release);
        Ok(())
    }

    fn snapshot(&self, writer: &mut dyn std::io::Write) -> Result<()> {
        let applied_index = self
            .control
            .lock()
            .expect("Sift control state lock poisoned")
            .applied_index;
        write_snapshot(&self.journal, applied_index, writer)?;
        Ok(())
    }

    fn snapshot_at(&self, index: Index, writer: &mut dyn std::io::Write) -> Result<()> {
        if let Some(checkpoint) = self.archive_checkpoint_for(index)? {
            return write_archive_checkpoint(&checkpoint, writer);
        }
        if let Some(checkpoint) = self.local_checkpoint_for(index)? {
            return write_local_checkpoint(&checkpoint, writer);
        }
        if let Some(checkpoint) = self.resident_checkpoint_for(index) {
            return write_resident_checkpoint(&checkpoint, writer);
        }
        if index != self.applied_index() {
            bail!(
                "Sift cannot snapshot Raft prefix {index}; current applied index is {}",
                self.applied_index()
            );
        }
        self.snapshot(writer)
    }

    fn validate_snapshot(&self, reader: &mut dyn Read) -> Result<()> {
        let stage = validate_streamed_snapshot(&self.journal, reader)?;
        *self
            .validated_archive_stage
            .lock()
            .expect("Sift validated archive stage lock poisoned") = stage;
        Ok(())
    }

    fn restore(&self, reader: &mut dyn std::io::Read) -> Result<()> {
        let current_applied_index = self.applied_index.load(Ordering::Acquire);
        let staged_archive = self
            .validated_archive_stage
            .lock()
            .expect("Sift validated archive stage lock poisoned")
            .take();
        let snapshot = restore_streamed_snapshot(
            &self.journal,
            current_applied_index,
            reader,
            staged_archive,
        )?;
        let current_pending_retention = self
            .control
            .lock()
            .expect("Sift control state lock poisoned")
            .pending_retention
            .clone();
        let pending_retention = match snapshot.pending_retention.clone() {
            Some(None)
                if current_pending_retention.as_ref().is_none_or(|fence| {
                    fence.target_generation <= self.journal.retention_generation()
                }) =>
            {
                None
            }
            Some(Some(snapshot_fence)) => {
                merge_pending_retention(current_pending_retention, Some(snapshot_fence))?
            }
            _ => current_pending_retention,
        };
        if snapshot.applied_index <= current_applied_index {
            let mut control = self
                .control
                .lock()
                .expect("Sift control state lock poisoned");
            if control.pending_retention != pending_retention {
                control.pending_retention = pending_retention;
                persist_control(&self.control_path, &control)?;
            }
            self.journal
                .set_retention_fenced(control.pending_retention.is_some());
            return Ok(());
        }
        let retention_fenced = pending_retention.is_some();
        let restored = ControlState {
            format_version: CONTROL_STATE_FORMAT_VERSION,
            applied_index: snapshot.applied_index,
            pending_retention,
        };
        persist_control(&self.control_path, &restored)?;
        *self
            .control
            .lock()
            .expect("Sift control state lock poisoned") = restored;
        self.journal.set_retention_fenced(retention_fenced);
        self.applied_index
            .store(snapshot.applied_index, Ordering::Release);
        *self
            .checkpoint_position
            .lock()
            .expect("Sift checkpoint position lock poisoned") = CheckpointPosition {
            applied_index: snapshot.applied_index,
            raw_cursor: self.journal.last_cursor(),
        };
        Ok(())
    }

    fn applied_index(&self) -> Index {
        self.applied_index.load(Ordering::Acquire)
    }
}
