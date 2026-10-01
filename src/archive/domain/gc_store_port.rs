//! GcStorePort isolates archive I/O.

use crate::archive::domain::control_values::ArchiveGcPending;
use crate::archive::domain::control_values::LocalBlobGcPending;
use crate::shared_kernel::archive_receipts::ArchiveReceipt;
use std::path::Path;

pub(crate) trait GcStorePort: Send + Sync {
    type Error;
    fn stage_archive_gc_pending(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<(), Self::Error>;
    fn promote_archive_gc_pending(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<(), Self::Error>;
    fn reconcile_staged_archive_gc(&self, root: &Path) -> Result<(), Self::Error>;
    fn install_archive_gc_plan(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<(), Self::Error>;
    fn withhold_archive_gc_plan(&self, root: &Path) -> Result<(), Self::Error>;
    fn archive_gc_pending(&self, root: &Path) -> bool;
    fn read_archive_gc_pending(&self, path: &Path) -> Result<ArchiveGcPending, Self::Error>;
    fn persist_archive_gc_pending(
        &self,
        path: &Path,
        pending: &ArchiveGcPending,
    ) -> Result<(), Self::Error>;
    fn remove_archive_gc_pending(&self, path: &Path) -> Result<(), Self::Error>;
    fn ensure_local_blob_gc_pending(&self, root: &Path) -> Result<(), Self::Error>;
    fn complete_local_blob_gc_plan(
        &self,
        root: &Path,
        pending_path: &Path,
        pending: &LocalBlobGcPending,
    ) -> Result<(), Self::Error>;
    fn read_local_blob_gc_pending(&self, path: &Path) -> Result<LocalBlobGcPending, Self::Error>;
    fn persist_local_blob_gc_pending(
        &self,
        path: &Path,
        pending: &LocalBlobGcPending,
    ) -> Result<(), Self::Error>;
    fn mark_local_blob_live(
        &self,
        root: &Path,
        replacement_sha256: &str,
        hash: &str,
    ) -> Result<(), Self::Error>;
    fn local_blob_is_live(
        &self,
        root: &Path,
        replacement_sha256: &str,
        hash: &str,
    ) -> Result<bool, Self::Error>;
}
