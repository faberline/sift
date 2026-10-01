//! Archive use cases receive these I/O services from assembly.

use crate::archive::domain::control_values::ArchiveCommitState;
use crate::archive::domain::control_values::ArchiveGcPending;
use crate::archive::domain::control_values::ArchiveUploadIntent;
use crate::archive::domain::control_values::LocalBlobGcPending;
use crate::archive::domain::control_values::LocalSegmentCommitState;
use crate::archive::domain::control_values::RestoreState;
use crate::shared_kernel::archive_manifest::ArchiveDedupeReceipt;
use crate::shared_kernel::archive_manifest::ArchiveManifest;
use crate::shared_kernel::archive_manifest::ArchiveSegment;
use crate::shared_kernel::archive_receipts::ArchiveReceipt;
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::shared_kernel::dedupe_receipt::DedupeReceipt;
use crate::shared_kernel::stored_event::StoredEvent;
use anyhow::Result;
use std::path::Path;

pub(crate) struct ArchiveServices {
    pub(crate) restore_target:
        crate::journal::application::restore_target_service::RestoreTargetService,
    pub(crate) resources: crate::archive::application::resource_factory::ResourceFactory,
    pub(crate) commit_store: std::sync::Arc<
        dyn crate::archive::domain::commit_store_port::CommitStorePort<Error = anyhow::Error>,
    >,
    pub(crate) upload_store: std::sync::Arc<
        dyn crate::archive::domain::upload_store_port::UploadStorePort<Error = anyhow::Error>,
    >,
    pub(crate) gc_store: std::sync::Arc<
        dyn crate::archive::domain::gc_store_port::GcStorePort<Error = anyhow::Error>,
    >,
    pub(crate) manifest_store: std::sync::Arc<
        dyn crate::archive::domain::manifest_store_port::ManifestStorePort<Error = anyhow::Error>,
    >,
    pub(crate) segment_codec: std::sync::Arc<
        dyn crate::archive::domain::segment_codec_port::SegmentCodecPort<
            Error = anyhow::Error,
            PartitionError = storage_segment::SegmentError,
        >,
    >,
    pub(crate) restore_store: std::sync::Arc<
        dyn crate::archive::domain::restore_store_port::RestoreStorePort<Error = anyhow::Error>,
    >,
    pub(crate) file_modes: std::sync::Arc<
        dyn crate::archive::domain::file_modes_port::FileModesPort<Error = anyhow::Error>,
    >,
}

impl ArchiveServices {
    pub(crate) fn read_commit_state(&self, root: &Path) -> Result<Option<ArchiveCommitState>> {
        self.commit_store.read_commit_state(root)
    }
    pub(crate) fn record_archive_commit(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<ArchiveWatermarks> {
        self.commit_store.record_archive_commit(root, receipt)
    }
    pub(crate) fn adopt_verified_archive_receipt(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<ArchiveWatermarks> {
        self.commit_store
            .adopt_verified_archive_receipt(root, receipt)
    }
    pub(crate) fn read_local_commit_state(
        &self,
        root: &Path,
    ) -> Result<Option<LocalSegmentCommitState>> {
        self.commit_store.read_local_commit_state(root)
    }
    pub(crate) fn retire_local_commit(&self, root: &Path) -> Result<()> {
        self.commit_store.retire_local_commit(root)
    }
    pub(crate) fn prepare_archive_upload_intent(
        &self,
        root: &Path,
        destination_uri: &str,
        destination_prefix: &str,
        cluster_id: &str,
        captured_cursor: u64,
        previous: Option<&ArchiveCommitState>,
    ) -> Result<ArchiveUploadIntent> {
        self.upload_store.prepare_archive_upload_intent(
            root,
            destination_uri,
            destination_prefix,
            cluster_id,
            captured_cursor,
            previous,
        )
    }
    pub(crate) fn clear_archive_upload_intent(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<()> {
        self.upload_store.clear_archive_upload_intent(root, receipt)
    }
    pub(crate) fn stage_archive_gc_pending(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<()> {
        self.gc_store.stage_archive_gc_pending(root, receipt)
    }
    pub(crate) fn promote_archive_gc_pending(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<()> {
        self.gc_store.promote_archive_gc_pending(root, receipt)
    }
    pub(crate) fn reconcile_staged_archive_gc(&self, root: &Path) -> Result<()> {
        self.gc_store.reconcile_staged_archive_gc(root)
    }
    pub(crate) fn install_archive_gc_plan(
        &self,
        root: &Path,
        receipt: &ArchiveReceipt,
    ) -> Result<()> {
        self.gc_store.install_archive_gc_plan(root, receipt)
    }
    pub(crate) fn withhold_archive_gc_plan(&self, root: &Path) -> Result<()> {
        self.gc_store.withhold_archive_gc_plan(root)
    }
    pub(crate) fn archive_gc_pending(&self, root: &Path) -> bool {
        self.gc_store.archive_gc_pending(root)
    }
    pub(crate) fn read_archive_gc_pending(&self, path: &Path) -> Result<ArchiveGcPending> {
        self.gc_store.read_archive_gc_pending(path)
    }
    pub(crate) fn persist_archive_gc_pending(
        &self,
        path: &Path,
        pending: &ArchiveGcPending,
    ) -> Result<()> {
        self.gc_store.persist_archive_gc_pending(path, pending)
    }
    pub(crate) fn remove_archive_gc_pending(&self, path: &Path) -> Result<()> {
        self.gc_store.remove_archive_gc_pending(path)
    }
    pub(crate) fn ensure_local_blob_gc_pending(&self, root: &Path) -> Result<()> {
        self.gc_store.ensure_local_blob_gc_pending(root)
    }
    pub(crate) fn complete_local_blob_gc_plan(
        &self,
        root: &Path,
        pending_path: &Path,
        pending: &LocalBlobGcPending,
    ) -> Result<()> {
        self.gc_store
            .complete_local_blob_gc_plan(root, pending_path, pending)
    }
    pub(crate) fn read_local_blob_gc_pending(&self, path: &Path) -> Result<LocalBlobGcPending> {
        self.gc_store.read_local_blob_gc_pending(path)
    }
    pub(crate) fn persist_local_blob_gc_pending(
        &self,
        path: &Path,
        pending: &LocalBlobGcPending,
    ) -> Result<()> {
        self.gc_store.persist_local_blob_gc_pending(path, pending)
    }
    pub(crate) fn mark_local_blob_live(
        &self,
        root: &Path,
        replacement_sha256: &str,
        hash: &str,
    ) -> Result<()> {
        self.gc_store
            .mark_local_blob_live(root, replacement_sha256, hash)
    }
    pub(crate) fn local_blob_is_live(
        &self,
        root: &Path,
        replacement_sha256: &str,
        hash: &str,
    ) -> Result<bool> {
        self.gc_store
            .local_blob_is_live(root, replacement_sha256, hash)
    }
    pub(crate) fn fetch_verified_committed_root(
        &self,
        root: &Path,
    ) -> Result<Option<ArchiveManifest>> {
        self.manifest_store.fetch_verified_committed_root(root)
    }
    pub(crate) fn cached_segment_bytes(
        &self,
        root: &Path,
        segment: &ArchiveSegment,
    ) -> Result<Vec<u8>> {
        self.manifest_store.cached_segment_bytes(root, segment)
    }
    pub(crate) fn fetch_dedupe_receipts(
        &self,
        receipt: &ArchiveDedupeReceipt,
    ) -> Result<Vec<DedupeReceipt>> {
        self.manifest_store.fetch_dedupe_receipts(receipt)
    }
    pub(crate) fn encode_parquet(&self, events: &[StoredEvent]) -> Result<Vec<u8>> {
        self.segment_codec.encode_parquet(events)
    }
    pub(crate) fn decode_parquet(&self, bytes: &[u8]) -> Result<Vec<StoredEvent>> {
        self.segment_codec.decode_parquet(bytes)
    }
    pub(crate) fn verify_archive_segment(
        &self,
        segment: &ArchiveSegment,
        events: &[StoredEvent],
    ) -> Result<()> {
        self.segment_codec.verify_archive_segment(segment, events)
    }
    pub(crate) fn read_restore_state(&self, path: &Path) -> Result<Option<RestoreState>> {
        self.restore_store.read_restore_state(path)
    }
    pub(crate) fn persist_restore_state(&self, path: &Path, state: &RestoreState) -> Result<()> {
        self.restore_store.persist_restore_state(path, state)
    }
    pub(crate) fn validate_building_restore_target(&self, target: &Path) -> Result<()> {
        self.restore_store.validate_building_restore_target(target)
    }
    pub(crate) fn publish_restore_stage(&self, target: &Path) -> Result<()> {
        self.restore_store.publish_restore_stage(target)
    }
    pub(crate) fn remove_restore_stage(&self, stage: &Path) -> Result<()> {
        self.restore_store.remove_restore_stage(stage)
    }
    pub(crate) fn require_empty_restore_target(&self, target: &Path) -> Result<()> {
        self.restore_store.require_empty_restore_target(target)
    }
    pub(crate) fn set_private_file(&self, path: &Path) -> Result<()> {
        self.file_modes.set_private_file(path)
    }
    pub(crate) fn set_private_dir(&self, path: &Path) -> Result<()> {
        self.file_modes.set_private_dir(path)
    }
}
