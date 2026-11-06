//! ManifestStorePort isolates archive I/O.

use crate::shared_kernel::archive_manifest::ArchiveDedupeReceipt;
use crate::shared_kernel::archive_manifest::ArchiveManifest;
use crate::shared_kernel::archive_manifest::ArchiveSegment;
use crate::shared_kernel::dedupe_receipt::DedupeReceipt;
use std::path::Path;

pub(crate) trait ManifestStorePort: Send + Sync {
    type Error;
    fn fetch_verified_committed_root(
        &self,
        root: &Path,
    ) -> Result<Option<ArchiveManifest>, Self::Error>;
    fn cached_segment_bytes(
        &self,
        root: &Path,
        segment: &ArchiveSegment,
    ) -> Result<Vec<u8>, Self::Error>;
    fn fetch_dedupe_receipts(
        &self,
        receipt: &ArchiveDedupeReceipt,
    ) -> Result<Vec<DedupeReceipt>, Self::Error>;
}
