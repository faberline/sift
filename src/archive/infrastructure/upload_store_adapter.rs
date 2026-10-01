//! Use the existing archive I/O operations and error chains.

use crate::archive::domain::control_values::ArchiveCommitState;
use crate::archive::domain::control_values::ArchiveUploadIntent;
use crate::archive::domain::upload_store_port::UploadStorePort;
use crate::shared_kernel::archive_receipts::ArchiveReceipt;
use anyhow::Result;
use std::path::Path;

pub(crate) struct NativeUploadStorePort;

impl UploadStorePort for NativeUploadStorePort {
    type Error = anyhow::Error;
    fn prepare_archive_upload_intent(
        &self,
        root: &Path,
        destination_uri: &str,
        destination_prefix: &str,
        cluster_id: &str,
        captured_cursor: u64,
        previous: Option<&ArchiveCommitState>,
    ) -> Result<ArchiveUploadIntent> {
        crate::archive::infrastructure::archive_upload_intent::prepare_archive_upload_intent(
            root,
            destination_uri,
            destination_prefix,
            cluster_id,
            captured_cursor,
            previous,
        )
    }
    fn clear_archive_upload_intent(&self, root: &Path, receipt: &ArchiveReceipt) -> Result<()> {
        crate::archive::infrastructure::archive_upload_intent::clear_archive_upload_intent(
            root, receipt,
        )
    }
}
