//! The archive checkpoint record: writing, reading, validating and restoring
//! it.

use std::io::{Read, Write};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::journal::domain::append_policy::validate_retention_fence;
use crate::journal::domain::retention_fence::RetentionFenceV1;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::journal::infrastructure::raft::archive_checkpoint_install::{
    archive_checkpoint_install_mode, ArchiveCheckpointInstallMode, ValidatedArchiveStage,
};
use crate::journal::infrastructure::raft::snapshot_format::{read_one_or_eof, SnapshotMetadata};

pub(super) const ARCHIVE_CHECKPOINT_MAGIC: &[u8; 8] = b"SIFTRCP1";

pub(super) const ARCHIVE_CHECKPOINT_FORMAT_VERSION: u16 = 2;

pub(super) const ARCHIVE_CHECKPOINT_HEADER_BYTES: usize = 16;

pub(super) const MAX_ARCHIVE_CHECKPOINT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArchiveCheckpointV1 {
    pub(super) format_version: u16,
    pub(super) applied_index: u64,
    pub(super) raw_cursor: u64,
    pub(super) archive_snapshot_index: u64,
    pub(super) watermarks: crate::shared_kernel::archive_watermarks::ArchiveWatermarks,
    pub(super) manifest_uri: String,
    pub(super) manifest_sha256: String,
    pub(super) retention_generation: u64,
    pub(super) pending_retention: Option<RetentionFenceV1>,
    /// Only an all-voter checkpoint may copy the manifest's object-deletion
    /// plan to a replica. A quorum-only checkpoint always keeps old objects.
    pub(super) archive_gc_authorized: bool,
}

pub(super) fn write_archive_checkpoint(
    checkpoint: &ArchiveCheckpointV1,
    writer: &mut dyn Write,
) -> Result<()> {
    validate_archive_checkpoint(checkpoint)?;
    let payload = serde_json::to_vec(checkpoint).context("encode Sift archive checkpoint")?;
    if payload.is_empty() || payload.len() > MAX_ARCHIVE_CHECKPOINT_BYTES {
        bail!("Sift archive checkpoint payload has an invalid length");
    }
    let payload_len =
        u32::try_from(payload.len()).context("archive checkpoint length exceeds u32")?;
    writer
        .write_all(ARCHIVE_CHECKPOINT_MAGIC)
        .context("write Sift archive checkpoint magic")?;
    writer
        .write_all(&payload_len.to_le_bytes())
        .context("write Sift archive checkpoint length")?;
    writer
        .write_all(&crc32fast::hash(&payload).to_le_bytes())
        .context("write Sift archive checkpoint checksum")?;
    writer
        .write_all(&payload)
        .context("write Sift archive checkpoint payload")
}

pub(super) fn read_archive_checkpoint(reader: &mut dyn Read) -> Result<ArchiveCheckpointV1> {
    let mut header = [0_u8; ARCHIVE_CHECKPOINT_HEADER_BYTES];
    reader
        .read_exact(&mut header)
        .context("read Sift archive checkpoint header")?;
    if &header[..8] != ARCHIVE_CHECKPOINT_MAGIC {
        bail!("invalid Sift archive checkpoint magic");
    }
    let payload_len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    let expected_checksum = u32::from_le_bytes(header[12..16].try_into().unwrap());
    if payload_len == 0 || payload_len > MAX_ARCHIVE_CHECKPOINT_BYTES {
        bail!("Sift archive checkpoint payload has an invalid length");
    }
    let mut payload = vec![0_u8; payload_len];
    reader
        .read_exact(&mut payload)
        .context("read Sift archive checkpoint payload")?;
    if crc32fast::hash(&payload) != expected_checksum {
        bail!("Sift archive checkpoint checksum mismatch");
    }
    if read_one_or_eof(reader)?.is_some() {
        bail!("Sift archive checkpoint contains trailing bytes");
    }
    let checkpoint: ArchiveCheckpointV1 =
        serde_json::from_slice(&payload).context("decode Sift archive checkpoint")?;
    validate_archive_checkpoint(&checkpoint)?;
    Ok(checkpoint)
}

pub(super) fn validate_archive_checkpoint(checkpoint: &ArchiveCheckpointV1) -> Result<()> {
    if checkpoint.format_version != ARCHIVE_CHECKPOINT_FORMAT_VERSION {
        bail!(
            "unsupported Sift archive checkpoint format {}",
            checkpoint.format_version
        );
    }
    if checkpoint.applied_index == 0
        || checkpoint.raw_cursor != checkpoint.archive_snapshot_index
        || checkpoint.watermarks.max_cursor() > checkpoint.archive_snapshot_index
        || !checkpoint.manifest_uri.starts_with("gs://")
        || checkpoint.manifest_sha256.len() != 64
        || !checkpoint
            .manifest_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("Sift archive checkpoint metadata is invalid");
    }
    if let Some(fence) = &checkpoint.pending_retention {
        validate_retention_fence(fence)?;
    }
    Ok(())
}

pub(super) fn restore_archive_checkpoint(
    journal: &DurableJournal,
    checkpoint: &ArchiveCheckpointV1,
    staged_archive: Option<ValidatedArchiveStage>,
) -> Result<SnapshotMetadata> {
    let manifest_bytes = service_backup::fetch_backup_object(&checkpoint.manifest_uri)
        .context("fetch Sift archive checkpoint manifest")?;
    let actual_hash = hex::encode(sha2::Sha256::digest(&manifest_bytes));
    if actual_hash != checkpoint.manifest_sha256 {
        bail!("Sift archive checkpoint manifest failed its SHA-256 check");
    }
    let expected_manifest: crate::archive::domain::archive_manifest::ArchiveManifest =
        serde_json::from_slice(&manifest_bytes)
            .context("decode Sift archive checkpoint manifest")?;
    crate::archive::domain::archive_manifest_validator::validate_archive_manifest(
        &expected_manifest,
    )?;
    if expected_manifest.raft_snapshot_index != checkpoint.archive_snapshot_index
        || expected_manifest.watermarks != checkpoint.watermarks
        || expected_manifest.raft_snapshot_index != checkpoint.raw_cursor
        || expected_manifest.retention_generation != checkpoint.retention_generation
    {
        bail!("Sift archive checkpoint is not covered by its manifest");
    }

    let install_mode = archive_checkpoint_install_mode(journal, checkpoint, &expected_manifest)?;
    let receipt = crate::archive::application::archive_receipts::ArchiveReceipt {
        manifest_uri: checkpoint.manifest_uri.clone(),
        manifest_sha256: checkpoint.manifest_sha256.clone(),
        manifest: expected_manifest.clone(),
    };
    if install_mode == ArchiveCheckpointInstallMode::FullRestore {
        let restored_root = match staged_archive {
            Some(stage)
                if stage.checkpoint == *checkpoint && stage.manifest == expected_manifest =>
            {
                stage.root
            }
            Some(_) => {
                bail!("Sift archive checkpoint restore lacks its validated staging directory")
            }
            None => {
                let restore_parent = journal.data_dir().join("tmp");
                let restored_root = tempfile::tempdir_in(&restore_parent).with_context(|| {
                    format!(
                        "create archive checkpoint restore directory in {}",
                        restore_parent.display()
                    )
                })?;
                let restored_manifest = crate::archive::application::restore_gcs::restore_gcs(
                    &checkpoint.manifest_uri,
                    restored_root.path(),
                )
                .context("restore Sift archive checkpoint into an isolated data root")?;
                if restored_manifest != expected_manifest {
                    bail!("Sift archive checkpoint manifest changed during restore");
                }
                restored_root
            }
        };
        let restored = DurableJournal::open(restored_root.path())
            .context("open isolated Sift archive checkpoint journal")?;
        journal.adopt_archive_checkpoint(&restored, &receipt, checkpoint.raw_cursor)?;
    } else if install_mode == ArchiveCheckpointInstallMode::RetentionDelta {
        journal.adopt_archive_retention_delta(&receipt, checkpoint.raw_cursor)?;
    } else {
        journal.adopt_archive_coverage(&receipt, checkpoint.raw_cursor)?;
    }
    if checkpoint.archive_gc_authorized {
        crate::archive::infrastructure::archive_gc_pending_store::install_archive_gc_plan(
            journal.data_dir(),
            &receipt,
        )?;
        // Every voter must finish its local content-addressed blob plan before
        // it acknowledges this all-voter checkpoint. The leader can then
        // delete the remote plan pages without stranding a follower.
        crate::archive::application::resume_local_blob_gc::finish_local_blob_gc(journal)?;
    } else {
        crate::archive::infrastructure::archive_gc_pending_store::withhold_archive_gc_plan(
            journal.data_dir(),
        )?;
    }
    crate::archive::application::evict_cold_segments::evict_committed_cold_segments_at(
        journal,
        chrono::Utc::now(),
    )?;
    Ok(SnapshotMetadata {
        applied_index: checkpoint.applied_index,
        last_cursor: expected_manifest.raft_snapshot_index,
        event_count: expected_manifest.event_count,
        pending_retention: Some(checkpoint.pending_retention.clone()),
    })
}
