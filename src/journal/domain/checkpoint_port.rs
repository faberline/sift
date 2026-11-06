//! Journal checkpoint operations without a Raft state-machine implementation.

use crate::shared_kernel::archive_receipts::ExpirationReceipt;
use crate::shared_kernel::checkpoint_data::CapturedPrefix;
use crate::shared_kernel::retention_fence::RetentionFenceV1;
use chrono::{DateTime, Utc};

pub(crate) trait JournalCheckpointPort: Send + Sync {
    type Error;
    fn pending_retention_fence(&self) -> Option<(RetentionFenceV1, u64)>;
    fn capture_archive_prefix(&self) -> Result<CapturedPrefix, Self::Error>;
    fn expire_current_archive_at(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<ExpirationReceipt>, Self::Error>;
    fn prepare_archive_checkpoint(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<(), Self::Error>;
    fn prepare_archive_checkpoint_without_gc(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<(), Self::Error>;
    fn prepare_local_checkpoint(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<(), Self::Error>;
    fn prepare_resident_checkpoint(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<(), Self::Error>;
}
