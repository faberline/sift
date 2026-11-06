//! UploadStorePort isolates archive I/O.

use crate::archive::domain::control_values::ArchiveCommitState;
use crate::archive::domain::control_values::ArchiveUploadIntent;
use crate::shared_kernel::archive_receipts::ArchiveReceipt;
use std::path::Path;

pub(crate) trait UploadStorePort: Send + Sync {
    type Error;
    fn prepare_archive_upload_intent(
        &self,
        root: &Path,
        destination_uri: &str,
        destination_prefix: &str,
        cluster_id: &str,
        captured_cursor: u64,
        previous: Option<&ArchiveCommitState>,
    ) -> Result<ArchiveUploadIntent, Self::Error>;
    fn clear_archive_upload_intent(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<(), Self::Error>;
}
