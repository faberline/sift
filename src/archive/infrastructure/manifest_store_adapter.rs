//! Use the existing archive I/O operations and error chains.

use crate::archive::domain::manifest_store_port::ManifestStorePort;
use crate::shared_kernel::archive_manifest::ArchiveDedupeReceipt;
use crate::shared_kernel::archive_manifest::ArchiveManifest;
use crate::shared_kernel::archive_manifest::ArchiveSegment;
use crate::shared_kernel::dedupe_receipt::DedupeReceipt;
use anyhow::Result;
use std::path::Path;

pub(crate) struct NativeManifestStorePort;

impl ManifestStorePort for NativeManifestStorePort {
    type Error = anyhow::Error;
    fn fetch_verified_committed_root(&self, root: &Path) -> Result<Option<ArchiveManifest>> {
        crate::archive::infrastructure::verified_manifest_fetcher::fetch_verified_committed_root(
            root,
        )
    }
    fn cached_segment_bytes(&self, root: &Path, segment: &ArchiveSegment) -> Result<Vec<u8>> {
        crate::archive::infrastructure::archive_segment_cache::cached_segment_bytes(root, segment)
    }
    fn fetch_dedupe_receipts(&self, receipt: &ArchiveDedupeReceipt) -> Result<Vec<DedupeReceipt>> {
        crate::archive::infrastructure::dedupe_receipt_archive::fetch_dedupe_receipts(receipt)
    }
}
