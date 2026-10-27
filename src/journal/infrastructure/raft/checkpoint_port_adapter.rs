//! Checkpoint operations use the existing state machine and apply gate.

use crate::journal::domain::checkpoint_port::JournalCheckpointPort;
use crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine;
use crate::shared_kernel::archive_receipts::ExpirationReceipt;
use crate::shared_kernel::checkpoint_data::CapturedPrefix;
use crate::shared_kernel::retention_fence::RetentionFenceV1;
use chrono::{DateTime, Utc};

impl JournalCheckpointPort for SiftStateMachine {
    type Error = anyhow::Error;
    fn pending_retention_fence(&self) -> Option<(RetentionFenceV1, u64)> {
        SiftStateMachine::pending_retention_fence(self)
    }
    fn capture_archive_prefix(&self) -> Result<CapturedPrefix, Self::Error> {
        SiftStateMachine::capture_archive_prefix(self)
    }
    fn expire_current_archive_at(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<ExpirationReceipt>, Self::Error> {
        SiftStateMachine::expire_current_archive_at(self, now)
    }
    fn prepare_archive_checkpoint(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<(), Self::Error> {
        SiftStateMachine::prepare_archive_checkpoint(self, applied_index, raw_cursor)
    }
    fn prepare_archive_checkpoint_without_gc(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<(), Self::Error> {
        SiftStateMachine::prepare_archive_checkpoint_without_gc(self, applied_index, raw_cursor)
    }
    fn prepare_local_checkpoint(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<(), Self::Error> {
        SiftStateMachine::prepare_local_checkpoint(self, applied_index, raw_cursor)
    }
    fn prepare_resident_checkpoint(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<(), Self::Error> {
        SiftStateMachine::prepare_resident_checkpoint(self, applied_index, raw_cursor)
    }
}
