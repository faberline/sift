//! Replacing the dedupe index with a rebuilt one through a staged, crash-safe
//! swap.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::journal::domain::dedupe_stats::DedupeStats;
use crate::journal::domain::idempotency_window::{generation_for, oldest_generation};
use crate::journal::infrastructure::storage::dedupe_generation_store::{
    load_state, stats_from_state,
};
use crate::journal::infrastructure::storage::dedupe_index::{DedupeIndex, DedupeState};
use crate::journal::infrastructure::storage::dedupe_shard_file::{
    set_private_dir, set_private_file,
};
use crate::journal::infrastructure::storage::dedupe_tree_copy::{
    copy_dedupe_tree, remove_file_if_exists, remove_real_dir_if_exists,
};

const REPLACE_FORMAT_VERSION: u32 = 1;

const REPLACE_MARKER_FILE: &str = ".dedupe-replace.json";

const REPLACE_STAGE_DIR: &str = ".dedupe-replace-stage";

const REPLACE_BACKUP_DIR: &str = ".dedupe-replace-backup";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReplaceIntent {
    format_version: u32,
    pub(super) indexed_through_cursor: u64,
    content_sha256: String,
}

impl DedupeIndex {
    pub(crate) fn replace_from(&self, source: &DedupeIndex) -> Result<DedupeStats> {
        let _source = source.state.read().expect("source dedupe lock poisoned");
        let mut target = self.state.write().expect("dedupe state lock poisoned");
        let now = Utc::now();
        reconcile_replace(&self.root, now)?;
        let paths = replace_paths(&self.root)?;
        remove_real_dir_if_exists(&paths.stage)?;
        remove_real_dir_if_exists(&paths.backup)?;
        copy_dedupe_tree(&source.root, &paths.stage)?;
        let staged = load_state(&paths.stage, generation_for(now))?;
        if staged.rebuild_required {
            bail!("replacement dedupe index has no durable metadata");
        }
        let intent = ReplaceIntent {
            format_version: REPLACE_FORMAT_VERSION,
            indexed_through_cursor: staged.indexed_through_cursor,
            content_sha256: hex::encode(staged.content_digest),
        };
        storage_durable::atomic_write(
            &paths.marker,
            &serde_json::to_vec(&intent)?,
            storage_durable::FsyncPolicy::Always,
        )?;
        set_private_file(&paths.marker)?;
        fs::rename(&self.root, &paths.backup).context("stage prior dedupe index backup")?;
        storage_durable::sync_parent_dir(&self.root)?;
        fs::rename(&paths.stage, &self.root).context("publish staged dedupe index")?;
        storage_durable::sync_parent_dir(&self.root)?;
        let published = load_intended_state(&self.root, &intent, now)?;
        remove_real_dir_if_exists(&paths.backup)?;
        remove_file_if_exists(&paths.marker)?;
        *target = published;
        Ok(stats_from_state(&target, oldest_generation(now)))
    }

    pub(crate) fn preflight_rebuild(&self) -> Result<()> {
        let metadata = fs::symlink_metadata(&self.root)
            .with_context(|| format!("inspect dedupe index root {}", self.root.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!(
                "dedupe index root {} is not a real directory",
                self.root.display()
            );
        }
        let probe = self.root.join(".rebuild-probe");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let file = options
            .open(&probe)
            .with_context(|| format!("write dedupe rebuild probe {}", probe.display()))?;
        file.sync_all()?;
        drop(file);
        fs::remove_file(&probe)?;
        storage_durable::sync_parent_dir(&probe)?;
        Ok(())
    }
}

struct ReplacePaths {
    marker: PathBuf,
    stage: PathBuf,
    backup: PathBuf,
}

fn replace_paths(root: &Path) -> Result<ReplacePaths> {
    let parent = root.parent().context("dedupe index root has no parent")?;
    Ok(ReplacePaths {
        marker: parent.join(REPLACE_MARKER_FILE),
        stage: parent.join(REPLACE_STAGE_DIR),
        backup: parent.join(REPLACE_BACKUP_DIR),
    })
}

pub(super) fn reconcile_replace(root: &Path, now: DateTime<Utc>) -> Result<()> {
    let parent = root.parent().context("dedupe index root has no parent")?;
    fs::create_dir_all(parent)?;
    set_private_dir(parent)?;
    let paths = replace_paths(root)?;
    if !paths.marker.exists() {
        if !root.exists() && paths.backup.exists() {
            fs::rename(&paths.backup, root).context("restore interrupted dedupe backup")?;
            storage_durable::sync_parent_dir(root)?;
        } else if root.exists() && paths.backup.exists() {
            load_state(root, generation_for(now))
                .context("validate published dedupe index before deleting backup")?;
            remove_real_dir_if_exists(&paths.backup)?;
        }
        remove_real_dir_if_exists(&paths.stage)?;
        return Ok(());
    }

    let marker_meta = fs::symlink_metadata(&paths.marker)?;
    if marker_meta.file_type().is_symlink() || !marker_meta.is_file() {
        bail!("dedupe replacement marker is not a regular file");
    }
    let intent: ReplaceIntent = serde_json::from_slice(&fs::read(&paths.marker)?)
        .context("decode dedupe replacement intent")?;
    validate_replace_intent(&intent)?;
    if root.exists() && load_intended_state(root, &intent, now).is_ok() {
        remove_real_dir_if_exists(&paths.stage)?;
        remove_real_dir_if_exists(&paths.backup)?;
        remove_file_if_exists(&paths.marker)?;
        return Ok(());
    }

    if paths.stage.exists() && load_intended_state(&paths.stage, &intent, now).is_ok() {
        if root.exists() {
            if paths.backup.exists() {
                remove_real_dir_if_exists(root)?;
            } else {
                fs::rename(root, &paths.backup)
                    .context("preserve prior dedupe index during recovery")?;
                storage_durable::sync_parent_dir(root)?;
            }
        }
        fs::rename(&paths.stage, root).context("finish staged dedupe index publication")?;
        storage_durable::sync_parent_dir(root)?;
        load_intended_state(root, &intent, now)?;
        remove_real_dir_if_exists(&paths.backup)?;
        remove_file_if_exists(&paths.marker)?;
        return Ok(());
    }

    if paths.backup.exists() {
        if root.exists() {
            remove_real_dir_if_exists(root)?;
        }
        fs::rename(&paths.backup, root).context("roll back interrupted dedupe replacement")?;
        storage_durable::sync_parent_dir(root)?;
        load_state(root, generation_for(now)).context("validate rolled-back dedupe index")?;
        remove_real_dir_if_exists(&paths.stage)?;
        remove_file_if_exists(&paths.marker)?;
        return Ok(());
    }

    bail!("dedupe replacement has no valid published, staged, or backup index")
}

fn validate_replace_intent(intent: &ReplaceIntent) -> Result<[u8; 32]> {
    if intent.format_version != REPLACE_FORMAT_VERSION {
        bail!(
            "unsupported dedupe replacement format {}",
            intent.format_version
        );
    }
    hex::decode(&intent.content_sha256)
        .context("decode dedupe replacement content digest")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("dedupe replacement content digest must be 32 bytes"))
}

fn load_intended_state(
    root: &Path,
    intent: &ReplaceIntent,
    now: DateTime<Utc>,
) -> Result<DedupeState> {
    let expected = validate_replace_intent(intent)?;
    let state = load_state(root, generation_for(now))?;
    if state.rebuild_required
        || state.indexed_through_cursor != intent.indexed_through_cursor
        || state.content_digest != expected
    {
        bail!("dedupe replacement does not match its intent");
    }
    Ok(state)
}
