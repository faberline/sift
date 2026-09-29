//! Validating a staged archive checkpoint and choosing how to install it.

use anyhow::{bail, Context, Result};
use sha2::Digest;

use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::journal::infrastructure::raft::archive_checkpoint::ArchiveCheckpointV1;

pub(super) struct ValidatedArchiveStage {
    pub(super) checkpoint: ArchiveCheckpointV1,
    pub(super) manifest: crate::archive::domain::archive_manifest::ArchiveManifest,
    pub(super) root: tempfile::TempDir,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ArchiveCheckpointInstallMode {
    Current,
    RetentionDelta,
    FullRestore,
}

pub(super) fn validate_archive_checkpoint_install(
    journal: &DurableJournal,
    checkpoint: &ArchiveCheckpointV1,
) -> Result<Option<ValidatedArchiveStage>> {
    let manifest_bytes = service_backup::fetch_backup_object(&checkpoint.manifest_uri)
        .context("fetch Sift archive checkpoint manifest during validation")?;
    let actual_hash = hex::encode(sha2::Sha256::digest(&manifest_bytes));
    if actual_hash != checkpoint.manifest_sha256 {
        bail!("Sift archive checkpoint manifest failed its SHA-256 check");
    }
    let manifest: crate::archive::domain::archive_manifest::ArchiveManifest =
        serde_json::from_slice(&manifest_bytes)
            .context("decode Sift archive checkpoint manifest during validation")?;
    crate::archive::domain::archive_manifest_validator::validate_archive_manifest(&manifest)?;
    if manifest.raft_snapshot_index != checkpoint.archive_snapshot_index
        || manifest.watermarks != checkpoint.watermarks
        || manifest.raft_snapshot_index != checkpoint.raw_cursor
        || manifest.retention_generation != checkpoint.retention_generation
    {
        bail!("Sift archive checkpoint is not covered by its manifest");
    }
    if archive_checkpoint_install_mode(journal, checkpoint, &manifest)?
        == ArchiveCheckpointInstallMode::FullRestore
    {
        let restored_root = tempfile::tempdir_in(journal.data_dir().join("tmp"))?;
        let restored = crate::archive::application::restore_gcs::restore_gcs(
            &checkpoint.manifest_uri,
            restored_root.path(),
        )
        .context("stage Sift archive checkpoint validation restore")?;
        if restored != manifest {
            bail!("Sift archive checkpoint manifest changed during validation");
        }
        let receipt = crate::archive::application::archive_receipts::ArchiveReceipt {
            manifest_uri: checkpoint.manifest_uri.clone(),
            manifest_sha256: checkpoint.manifest_sha256.clone(),
            manifest: manifest.clone(),
        };
        crate::archive::infrastructure::archive_commit_state::adopt_verified_archive_receipt(
            restored_root.path(),
            &receipt,
        )?;
        let staged = DurableJournal::open(restored_root.path())?;
        crate::archive::application::evict_cold_segments::evict_committed_cold_segments_at(
            &staged,
            chrono::Utc::now(),
        )?;
        drop(staged);
        return Ok(Some(ValidatedArchiveStage {
            checkpoint: checkpoint.clone(),
            manifest,
            root: restored_root,
        }));
    }
    Ok(None)
}

pub(super) fn archive_checkpoint_install_mode(
    journal: &DurableJournal,
    checkpoint: &ArchiveCheckpointV1,
    manifest: &crate::archive::domain::archive_manifest::ArchiveManifest,
) -> Result<ArchiveCheckpointInstallMode> {
    let empty = journal.last_cursor() == 0 && journal.total_event_count() == 0;
    if empty || journal.last_cursor() < checkpoint.raw_cursor {
        return Ok(ArchiveCheckpointInstallMode::FullRestore);
    }
    let local_status =
        crate::archive::application::archive_status_queries::committed_status(journal.data_dir())?;
    let local_generation = local_status
        .as_ref()
        .map(|status| status.retention_generation)
        .unwrap_or_default();
    if local_generation > checkpoint.retention_generation {
        bail!("Sift archive checkpoint retention generation moved backwards");
    }
    let suffix_events = journal.last_cursor().saturating_sub(checkpoint.raw_cursor);
    let (prefix_events, prefix_digest, prefix_generation) =
        journal.checkpoint_identity(checkpoint.raw_cursor)?;
    if prefix_generation == checkpoint.retention_generation
        && prefix_events == manifest.event_count
        && hex::encode(prefix_digest) == manifest.event_content_sha256
        && journal.total_event_count()
            == manifest
                .event_count
                .checked_add(suffix_events)
                .context("archive checkpoint event count exhausted u64")?
    {
        return Ok(ArchiveCheckpointInstallMode::Current);
    }
    let Some(delta) = &manifest.retention_delta else {
        return Ok(ArchiveCheckpointInstallMode::FullRestore);
    };
    let source_status_matches = local_status.as_ref().is_some_and(|status| {
        status.manifest_uri == delta.source_manifest_uri
            && status.manifest_sha256 == delta.source_manifest_sha256
            && status.retention_generation == delta.source_generation
    });
    let source_total = delta
        .source_event_count
        .checked_add(suffix_events)
        .context("retention delta source event count exhausted u64")?;
    if source_status_matches
        && prefix_generation == delta.source_generation
        && prefix_events == delta.source_event_count
        && hex::encode(prefix_digest) == delta.source_event_content_sha256
        && journal.total_event_count() == source_total
        && checkpoint.retention_generation == delta.source_generation.saturating_add(1)
    {
        return Ok(ArchiveCheckpointInstallMode::RetentionDelta);
    }
    Ok(ArchiveCheckpointInstallMode::FullRestore)
}
