//! Use the existing archive I/O operations and error chains.

use crate::archive::domain::control_values::ArchiveGcPending;
use crate::archive::domain::control_values::LocalBlobGcPending;
use crate::archive::domain::gc_store_port::GcStorePort;
use crate::shared_kernel::archive_receipts::ArchiveReceipt;
use anyhow::Result;
use std::path::Path;

pub(crate) struct NativeGcStorePort;

impl GcStorePort for NativeGcStorePort {
    type Error = anyhow::Error;
    fn stage_archive_gc_pending(&self, root: &Path, receipt: &ArchiveReceipt) -> Result<()> {
        crate::archive::infrastructure::archive_gc_pending_store::stage_archive_gc_pending(
            root, receipt,
        )
    }
    fn promote_archive_gc_pending(&self, root: &Path, receipt: &ArchiveReceipt) -> Result<()> {
        crate::archive::infrastructure::archive_gc_pending_store::promote_archive_gc_pending(
            root, receipt,
        )
    }
    fn reconcile_staged_archive_gc(&self, root: &Path) -> Result<()> {
        crate::archive::infrastructure::archive_gc_pending_store::reconcile_staged_archive_gc(root)
    }
    fn install_archive_gc_plan(&self, root: &Path, receipt: &ArchiveReceipt) -> Result<()> {
        crate::archive::infrastructure::archive_gc_pending_store::install_archive_gc_plan(
            root, receipt,
        )
    }
    fn withhold_archive_gc_plan(&self, root: &Path) -> Result<()> {
        crate::archive::infrastructure::archive_gc_pending_store::withhold_archive_gc_plan(root)
    }
    fn archive_gc_pending(&self, root: &Path) -> bool {
        crate::archive::infrastructure::archive_gc_pending_store::archive_gc_pending(root)
    }
    fn read_archive_gc_pending(&self, path: &Path) -> Result<ArchiveGcPending> {
        crate::archive::infrastructure::archive_gc_pending_store::read_archive_gc_pending(path)
    }
    fn persist_archive_gc_pending(&self, path: &Path, pending: &ArchiveGcPending) -> Result<()> {
        crate::archive::infrastructure::archive_gc_pending_store::persist_archive_gc_pending(
            path, pending,
        )
    }
    fn remove_archive_gc_pending(&self, path: &Path) -> Result<()> {
        crate::archive::infrastructure::archive_gc_pending_store::remove_archive_gc_pending(path)
    }
    fn ensure_local_blob_gc_pending(&self, root: &Path) -> Result<()> {
        crate::archive::infrastructure::local_blob_gc_store::ensure_local_blob_gc_pending(root)
    }
    fn complete_local_blob_gc_plan(
        &self,
        root: &Path,
        pending_path: &Path,
        pending: &LocalBlobGcPending,
    ) -> Result<()> {
        crate::archive::infrastructure::local_blob_gc_store::complete_local_blob_gc_plan(
            root,
            pending_path,
            pending,
        )
    }
    fn read_local_blob_gc_pending(&self, path: &Path) -> Result<LocalBlobGcPending> {
        crate::archive::infrastructure::local_blob_gc_store::read_local_blob_gc_pending(path)
    }
    fn persist_local_blob_gc_pending(
        &self,
        path: &Path,
        pending: &LocalBlobGcPending,
    ) -> Result<()> {
        crate::archive::infrastructure::local_blob_gc_store::persist_local_blob_gc_pending(
            path, pending,
        )
    }
    fn mark_local_blob_live(
        &self,
        root: &Path,
        replacement_sha256: &str,
        hash: &str,
    ) -> Result<()> {
        crate::archive::infrastructure::local_blob_live_index::mark_local_blob_live(
            root,
            replacement_sha256,
            hash,
        )
    }
    fn local_blob_is_live(
        &self,
        root: &Path,
        replacement_sha256: &str,
        hash: &str,
    ) -> Result<bool> {
        crate::archive::infrastructure::local_blob_live_index::local_blob_is_live(
            root,
            replacement_sha256,
            hash,
        )
    }
}
