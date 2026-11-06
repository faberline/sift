//! Journal checkpoint use cases over the same checkpoint port instance.

use crate::journal::domain::checkpoint_port::JournalCheckpointPort;
use crate::shared_kernel::archive_receipts::ExpirationReceipt;
use crate::shared_kernel::checkpoint_data::CapturedPrefix;
use crate::shared_kernel::retention_fence::RetentionFenceV1;
use chrono::{DateTime, Utc};
use std::sync::Arc;

pub(crate) struct JournalCheckpointService {
    port: Arc<dyn JournalCheckpointPort<Error = anyhow::Error>>,
}

impl JournalCheckpointService {
    pub(crate) fn new(port: Arc<dyn JournalCheckpointPort<Error = anyhow::Error>>) -> Self {
        Self { port }
    }
    pub(crate) fn pending_retention_fence(&self) -> Option<(RetentionFenceV1, u64)> {
        self.port.pending_retention_fence()
    }
    pub(crate) fn capture_archive_prefix(&self) -> Result<CapturedPrefix, anyhow::Error> {
        self.port.capture_archive_prefix()
    }
    pub(crate) fn expire_current_archive_at(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<ExpirationReceipt>, anyhow::Error> {
        self.port.expire_current_archive_at(now)
    }
    pub(crate) fn prepare_archive_checkpoint(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<(), anyhow::Error> {
        self.port
            .prepare_archive_checkpoint(applied_index, raw_cursor)
    }
    pub(crate) fn prepare_archive_checkpoint_without_gc(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<(), anyhow::Error> {
        self.port
            .prepare_archive_checkpoint_without_gc(applied_index, raw_cursor)
    }
    pub(crate) fn prepare_local_checkpoint(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<(), anyhow::Error> {
        self.port
            .prepare_local_checkpoint(applied_index, raw_cursor)
    }
    pub(crate) fn prepare_resident_checkpoint(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<(), anyhow::Error> {
        self.port
            .prepare_resident_checkpoint(applied_index, raw_cursor)
    }
}
