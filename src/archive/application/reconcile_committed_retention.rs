//! Reconciling the journal's retention with the committed archive.

use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::archive::application::archive_status_queries::committed_status;
use crate::archive::application::restore_gcs::restore_gcs;
use crate::archive::domain::event_set_digest::decode_event_content_digest;
use crate::archive::infrastructure::verified_manifest_fetcher::fetch_verified_committed_root;
use crate::journal::infrastructure::storage::raw_storage::RawStorage;

/// Finish a retention commit that reached the local archive receipt but was
/// interrupted before the local hot set and projection generation changed.
/// This runs only when the manifest generation is newer than the journal head.
pub(crate) fn reconcile_committed_retention(
    root: &Path,
    storage: &RawStorage,
    current_generation: u64,
) -> Result<Option<u64>> {
    let Some(status) = committed_status(root)? else {
        return Ok(None);
    };
    if status.retention_generation <= current_generation {
        return Ok(Some(status.retention_generation));
    }
    let committed = fetch_verified_committed_root(root)?
        .context("committed retention recovery requires its manifest")?;
    if let Some(delta) = &committed.retention_delta {
        if delta.source_generation == current_generation
            && committed.retention_generation == current_generation.saturating_add(1)
        {
            let cutoff = DateTime::<Utc>::from_timestamp_nanos(delta.cutoff_unix_nano);
            storage.evict_expired_before(cutoff, committed.raft_snapshot_index)?;
            return Ok(Some(committed.retention_generation));
        }
    }
    let restore_parent = root.join("tmp");
    let restored_root = tempfile::tempdir_in(&restore_parent).with_context(|| {
        format!(
            "create interrupted-retention recovery directory in {}",
            restore_parent.display()
        )
    })?;
    let manifest = restore_gcs(&status.manifest_uri, restored_root.path())
        .context("restore committed archive for interrupted retention recovery")?;
    if manifest.retention_generation != status.retention_generation
        || manifest.raft_snapshot_index != status.snapshot_index
    {
        bail!("committed retention manifest changed during recovery");
    }
    let retained = RawStorage::open(restored_root.path())?;
    storage.reconcile_retained_prefix(&retained, manifest.raft_snapshot_index)?;
    Ok(Some(manifest.retention_generation))
}

/// Finish a retention receipt inside the same process after a late local
/// failure. The remote manifest is already the durable authority. Reapply its
/// idempotent local delta and persist the journal head before serving again.
pub(crate) fn reconcile_live_committed_retention(
    journal: &crate::journal::infrastructure::durable_journal::DurableJournal,
) -> Result<()> {
    let current_generation = journal.retention_generation();
    let Some(status) = committed_status(journal.data_dir())? else {
        return Ok(());
    };
    if status.retention_generation < current_generation {
        bail!("local retention generation is ahead of its committed manifest");
    }
    if status.retention_generation == current_generation && !journal.recovery_required() {
        return Ok(());
    }
    let committed = fetch_verified_committed_root(journal.data_dir())?
        .context("live retention recovery requires its committed manifest")?;
    let delta = committed
        .retention_delta
        .as_ref()
        .context("live retention recovery requires a committed retention delta")?;
    if delta.source_generation.saturating_add(1) != committed.retention_generation
        || (current_generation != delta.source_generation
            && current_generation != committed.retention_generation)
    {
        bail!("live retention recovery cannot bridge the committed generation gap");
    }
    reconcile_committed_retention(journal.data_dir(), journal.storage(), current_generation)?;
    journal.apply_expiration_head(
        DateTime::<Utc>::from_timestamp_nanos(delta.cutoff_unix_nano),
        delta.source_event_count,
        committed.event_count,
        decode_event_content_digest(&delta.source_event_content_sha256)?,
        decode_event_content_digest(&committed.event_content_sha256)?,
        true,
    )
}
