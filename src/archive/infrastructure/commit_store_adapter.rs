//! Use the existing archive I/O operations and error chains.

use crate::archive::domain::commit_store_port::CommitStorePort;
use crate::archive::domain::control_values::ArchiveCommitState;
use crate::archive::domain::control_values::LocalSegmentCommitState;
use crate::shared_kernel::archive_receipts::ArchiveReceipt;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use anyhow::Result;
use std::path::Path;

pub(crate) struct NativeCommitStorePort;

impl CommitStorePort for NativeCommitStorePort {
    type Error = anyhow::Error;
    fn read_commit_state(&self, root: &Path) -> Result<Option<ArchiveCommitState>> {
        crate::archive::infrastructure::archive_commit_state::read_commit_state(root)
    }
    fn record_archive_commit(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<ArchiveWatermarks> {
        crate::archive::infrastructure::archive_commit_state::record_archive_commit(root, receipt)
    }
    fn adopt_verified_archive_receipt(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<ArchiveWatermarks> {
        crate::archive::infrastructure::archive_commit_state::adopt_verified_archive_receipt(
            root, receipt,
        )
    }
    fn read_local_commit_state(&self, root: &Path) -> Result<Option<LocalSegmentCommitState>> {
        crate::archive::infrastructure::local_segment_commit_state::read_local_commit_state(root)
    }
    fn retire_local_commit(&self, root: &Path) -> Result<()> {
        crate::archive::infrastructure::local_segment_commit_state::retire_local_commit(root)
    }
}
