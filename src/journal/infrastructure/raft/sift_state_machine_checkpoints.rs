//! Preparing archive, local and resident checkpoints from the state machine.

use anyhow::{bail, Context, Result};

use crate::journal::infrastructure::raft::archive_checkpoint::{
    validate_archive_checkpoint, ArchiveCheckpointV1, ARCHIVE_CHECKPOINT_FORMAT_VERSION,
};
use crate::journal::infrastructure::raft::local_checkpoint::{
    validate_local_checkpoint, LocalCheckpointV1, LOCAL_CHECKPOINT_FORMAT_VERSION,
};
use crate::journal::infrastructure::raft::resident_checkpoint::{
    validate_resident_checkpoint, ResidentCheckpointV1, RESIDENT_CHECKPOINT_FORMAT_VERSION,
};
use crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine;

impl SiftStateMachine {
    #[doc(hidden)]
    pub fn prepare_archive_checkpoint(&self, applied_index: u64, raw_cursor: u64) -> Result<()> {
        self.prepare_archive_checkpoint_with_gc(applied_index, raw_cursor, true)
    }

    /// Prepare a GCS-backed checkpoint that can bound the Raft log on quorum
    /// but cannot authorize deletion of any archive object.
    #[doc(hidden)]
    pub fn prepare_archive_checkpoint_without_gc(
        &self,
        applied_index: u64,
        raw_cursor: u64,
    ) -> Result<()> {
        self.prepare_archive_checkpoint_with_gc(applied_index, raw_cursor, false)
    }

    fn prepare_archive_checkpoint_with_gc(
        &self,
        applied_index: u64,
        raw_cursor: u64,
        archive_gc_authorized: bool,
    ) -> Result<()> {
        if applied_index == 0 {
            bail!("cannot checkpoint an empty Sift Raft prefix");
        }
        let status = crate::archive::application::archive_status_queries::committed_status(
            self.journal.data_dir(),
        )?
        .context("Sift Raft checkpoint requires a committed remote archive")?;
        let archive_snapshot_index = status.snapshot_index;
        if archive_snapshot_index != raw_cursor {
            bail!(
                "committed Sift archive cursor {archive_snapshot_index} does not equal Raft prefix cursor {raw_cursor}"
            );
        }
        let checkpoint = ArchiveCheckpointV1 {
            format_version: ARCHIVE_CHECKPOINT_FORMAT_VERSION,
            applied_index,
            raw_cursor,
            archive_snapshot_index,
            watermarks: status.watermarks,
            manifest_uri: status.manifest_uri,
            manifest_sha256: status.manifest_sha256,
            retention_generation: status.retention_generation,
            pending_retention: if archive_gc_authorized && !status.retention_scan_pending {
                None
            } else {
                self.control
                    .lock()
                    .expect("Sift control state lock poisoned")
                    .pending_retention
                    .clone()
            },
            archive_gc_authorized,
        };
        validate_archive_checkpoint(&checkpoint)?;
        *self
            .archive_checkpoint
            .lock()
            .expect("Sift archive checkpoint lock poisoned") = Some(checkpoint);
        Ok(())
    }

    #[doc(hidden)]
    pub fn prepare_local_checkpoint(&self, applied_index: u64, raw_cursor: u64) -> Result<()> {
        if applied_index == 0 {
            bail!("cannot checkpoint an empty Sift Raft prefix");
        }
        let status = crate::archive::application::archive_status_queries::local_committed_status(
            self.journal.data_dir(),
        )?
        .context("Sift Raft checkpoint requires a committed local segment set")?;
        if status.snapshot_index != raw_cursor {
            bail!(
                "committed local cursor {} does not equal Raft prefix cursor {raw_cursor}",
                status.snapshot_index
            );
        }
        let checkpoint = LocalCheckpointV1 {
            format_version: LOCAL_CHECKPOINT_FORMAT_VERSION,
            applied_index,
            raw_cursor,
            local_snapshot_index: status.snapshot_index,
            watermarks: status.watermarks,
            pending_retention: self
                .control
                .lock()
                .expect("Sift control state lock poisoned")
                .pending_retention
                .clone(),
        };
        validate_local_checkpoint(&checkpoint)?;
        *self
            .local_checkpoint
            .lock()
            .expect("Sift local checkpoint lock poisoned") = Some(checkpoint);
        Ok(())
    }

    /// Prepare a small Raft-only checkpoint backed by each voter's durable
    /// local journal. This never authorizes WAL or archive deletion.
    #[doc(hidden)]
    pub fn prepare_resident_checkpoint(&self, applied_index: u64, raw_cursor: u64) -> Result<()> {
        let _gate = self
            .commit_gate
            .lock()
            .expect("Sift commit gate lock poisoned");
        let position = *self
            .checkpoint_position
            .lock()
            .expect("Sift checkpoint position lock poisoned");
        if applied_index == 0
            || position.applied_index != applied_index
            || position.raw_cursor != raw_cursor
            || self.journal.last_cursor() != raw_cursor
        {
            bail!("Sift resident checkpoint moved while it was being prepared");
        }
        let (event_count, event_content_digest, retention_generation) =
            self.journal.checkpoint_identity(raw_cursor)?;
        let checkpoint = ResidentCheckpointV1 {
            format_version: RESIDENT_CHECKPOINT_FORMAT_VERSION,
            applied_index,
            raw_cursor,
            event_count,
            retention_generation,
            event_content_sha256: hex::encode(event_content_digest),
            pending_retention: self
                .control
                .lock()
                .expect("Sift control state lock poisoned")
                .pending_retention
                .clone(),
        };
        validate_resident_checkpoint(&checkpoint)?;
        *self
            .resident_checkpoint
            .lock()
            .expect("Sift resident checkpoint lock poisoned") = Some(checkpoint);
        Ok(())
    }

    pub(super) fn archive_checkpoint_for(&self, index: u64) -> Result<Option<ArchiveCheckpointV1>> {
        if let Some(checkpoint) = self
            .archive_checkpoint
            .lock()
            .expect("Sift archive checkpoint lock poisoned")
            .as_ref()
            .filter(|checkpoint| checkpoint.applied_index == index)
            .cloned()
        {
            return Ok(Some(checkpoint));
        }
        let (applied_index, raw_cursor) = self.checkpoint_position();
        if applied_index != index {
            return Ok(None);
        }
        if crate::archive::application::archive_status_queries::committed_status(
            self.journal.data_dir(),
        )?
        .is_none_or(|status| status.snapshot_index != raw_cursor)
        {
            return Ok(None);
        }
        self.prepare_archive_checkpoint(applied_index, raw_cursor)?;
        Ok(self
            .archive_checkpoint
            .lock()
            .expect("Sift archive checkpoint lock poisoned")
            .clone())
    }

    pub(super) fn local_checkpoint_for(&self, index: u64) -> Result<Option<LocalCheckpointV1>> {
        if let Some(checkpoint) = self
            .local_checkpoint
            .lock()
            .expect("Sift local checkpoint lock poisoned")
            .as_ref()
            .filter(|checkpoint| checkpoint.applied_index == index)
            .cloned()
        {
            return Ok(Some(checkpoint));
        }
        let (applied_index, raw_cursor) = self.checkpoint_position();
        if applied_index != index
            || crate::archive::application::archive_status_queries::local_committed_status(
                self.journal.data_dir(),
            )?
            .is_none_or(|status| status.snapshot_index != raw_cursor)
        {
            return Ok(None);
        }
        self.prepare_local_checkpoint(applied_index, raw_cursor)?;
        Ok(self
            .local_checkpoint
            .lock()
            .expect("Sift local checkpoint lock poisoned")
            .clone())
    }

    pub(super) fn resident_checkpoint_for(&self, index: u64) -> Option<ResidentCheckpointV1> {
        self.resident_checkpoint
            .lock()
            .expect("Sift resident checkpoint lock poisoned")
            .as_ref()
            .filter(|checkpoint| checkpoint.applied_index == index)
            .cloned()
    }
}
