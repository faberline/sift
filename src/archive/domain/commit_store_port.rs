//! CommitStorePort isolates archive I/O.

use crate::archive::domain::control_values::ArchiveCommitState;
use crate::archive::domain::control_values::LocalSegmentCommitState;
use crate::shared_kernel::archive_receipts::ArchiveReceipt;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use std::path::Path;

pub(crate) trait CommitStorePort: Send + Sync {
    type Error;
    fn read_commit_state(&self, root: &Path) -> Result<Option<ArchiveCommitState>, Self::Error>;
    fn record_archive_commit(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<ArchiveWatermarks, Self::Error>;
    fn adopt_verified_archive_receipt(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<ArchiveWatermarks, Self::Error>;
    fn read_local_commit_state(
        &self,
        root: &Path,
    ) -> Result<Option<LocalSegmentCommitState>, Self::Error>;
    fn retire_local_commit(&self, root: &Path) -> Result<(), Self::Error>;
}
