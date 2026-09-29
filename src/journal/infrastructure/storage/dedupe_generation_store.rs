//! The dedupe generations on disk: their directories, the meta record, loading
//! and sealing them, and appending entries.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;

use crate::journal::domain::bloom_filter::{BloomLayer, MIN_BLOOM_BYTES};
use crate::journal::domain::dedupe_stats::DedupeStats;
use crate::journal::domain::event_digest::{shard_for, xor_digest_in_place};
use crate::journal::domain::idempotency_window::IDEMPOTENCY_WINDOW_SECONDS;
use crate::journal::infrastructure::storage::dedupe_index::DedupeState;
use crate::journal::infrastructure::storage::dedupe_shard_file::{
    generation_shard_identity, load_generation_receipts, read_entries, rebuild_generation_shards,
    set_private_dir, set_private_file, write_entries_with_policy, ENTRY_BYTES, RECEIPT_LOG_FILE,
};

pub(super) const META_FILE: &str = "meta.json";

const FORMAT_VERSION: u32 = 6;

pub(super) struct GenerationState {
    pub(super) sealed: bool,
    pub(super) blooms: Vec<BloomLayer>,
    pub(super) entry_count: u64,
    pub(super) newest_cursor: u64,
    pub(super) newest_acknowledged_at_unix_nano: Option<i64>,
    pub(super) content_digest: [u8; 32],
    pub(super) receipt_writer: Option<storage_durable::FramedLogWriter>,
    pub(super) pending: BTreeMap<[u8; 32], (u64, i64)>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DedupeMeta {
    pub(super) format_version: u32,
    pub(super) indexed_through_cursor: u64,
    pub(super) content_sha256: String,
}

impl GenerationState {
    pub(super) fn empty(sealed: bool) -> Self {
        Self {
            sealed,
            blooms: vec![BloomLayer::new(MIN_BLOOM_BYTES)],
            entry_count: 0,
            newest_cursor: 0,
            newest_acknowledged_at_unix_nano: None,
            content_digest: [0; 32],
            receipt_writer: None,
            pending: BTreeMap::new(),
        }
    }
}

pub(super) fn generation_path(root: &Path, generation: i64) -> PathBuf {
    root.join(format!("g-{generation}"))
}

pub(super) fn parse_generation(name: &str) -> Result<i64> {
    name.strip_prefix("g-")
        .context("dedupe generation directory has an unknown name")?
        .parse::<i64>()
        .context("dedupe generation directory has an invalid number")
}

pub(super) fn shard_path(root: &Path, generation: i64, digest: &[u8; 32]) -> PathBuf {
    generation_path(root, generation).join(format!("{:03x}.idx", shard_for(digest)))
}

pub(super) fn load_state(root: &Path, current: i64) -> Result<DedupeState> {
    let mut state = DedupeState::default();
    let mut found_meta = false;
    let mut expected_content_digest = None::<[u8; 32]>;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() || (!file_type.is_dir() && !file_type.is_file()) {
            bail!("dedupe root contains a special file");
        }
        if file_type.is_file() {
            if entry.file_name() != META_FILE {
                bail!("legacy or damaged dedupe index requires a rebuild");
            }
            let meta: DedupeMeta = serde_json::from_slice(&fs::read(entry.path())?)
                .context("decode dedupe index metadata")?;
            if meta.format_version != FORMAT_VERSION {
                bail!("unsupported dedupe index format {}", meta.format_version);
            }
            state.indexed_through_cursor = meta.indexed_through_cursor;
            expected_content_digest = Some(
                hex::decode(&meta.content_sha256)
                    .context("decode dedupe index content digest")?
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("dedupe index content digest must be 32 bytes"))?,
            );
            found_meta = true;
            continue;
        }
        let name = entry.file_name();
        let generation = parse_generation(&name.to_string_lossy())?;
        set_private_dir(&entry.path())?;
        let sealed = generation < current;
        let loaded = load_generation(&entry.path(), sealed)?;
        state.applied_time_unix_nano = match (
            state.applied_time_unix_nano,
            loaded.newest_acknowledged_at_unix_nano,
        ) {
            (Some(current), Some(loaded)) => Some(current.max(loaded)),
            (None, loaded) => loaded,
            (current, None) => current,
        };
        xor_digest_in_place(&mut state.content_digest, loaded.content_digest);
        state.indexed_through_cursor = state.indexed_through_cursor.max(loaded.newest_cursor);
        state.generations.insert(generation, loaded);
    }
    if expected_content_digest.is_some_and(|expected| expected != state.content_digest) {
        bail!("dedupe index content digest mismatch");
    }
    state.rebuild_required = !found_meta;
    Ok(state)
}

fn load_generation(directory: &Path, sealed: bool) -> Result<GenerationState> {
    let receipt_path = directory.join(RECEIPT_LOG_FILE);
    let mut receipts = load_generation_receipts(directory, sealed)?;
    let (shard_count, shard_digest) = generation_shard_identity(directory)?;
    if shard_count != receipts.entry_count || shard_digest != receipts.content_digest {
        rebuild_generation_shards(directory, sealed)?;
    } else if sealed {
        seal_generation(directory)?;
    }
    if !sealed {
        receipts.receipt_writer = Some(storage_durable::FramedLogWriter::open(
            receipt_path,
            storage_durable::FsyncPolicy::Os,
        )?);
    }
    Ok(receipts)
}

pub(super) fn stats_from_state(state: &DedupeState, oldest: i64) -> DedupeStats {
    let mut stats = DedupeStats {
        indexed_through_cursor: state.indexed_through_cursor,
        window_seconds: IDEMPOTENCY_WINDOW_SECONDS as u64,
        rebuild_required: state.rebuild_required,
        ..DedupeStats::default()
    };
    for (generation, index) in state.generations.range(oldest..) {
        if stats.entry_count == 0 {
            stats.oldest_generation = *generation;
        }
        stats.newest_generation = *generation;
        stats.entry_count = stats.entry_count.saturating_add(index.entry_count);
        stats.newest_cursor = stats.newest_cursor.max(index.newest_cursor);
    }
    stats
}

pub(super) fn pending_entry_count(state: &DedupeState) -> usize {
    state.generations.values().fold(0_usize, |count, index| {
        count.saturating_add(index.pending.len())
    })
}

pub(super) fn persist_meta(
    root: &Path,
    indexed_through_cursor: u64,
    content_digest: [u8; 32],
) -> Result<()> {
    persist_meta_with_policy(
        root,
        indexed_through_cursor,
        content_digest,
        storage_durable::FsyncPolicy::Always,
    )
}

pub(super) fn persist_meta_with_policy(
    root: &Path,
    indexed_through_cursor: u64,
    content_digest: [u8; 32],
    policy: storage_durable::FsyncPolicy,
) -> Result<()> {
    let bytes = serde_json::to_vec(&DedupeMeta {
        format_version: FORMAT_VERSION,
        indexed_through_cursor,
        content_sha256: hex::encode(content_digest),
    })?;
    let path = root.join(META_FILE);
    storage_durable::atomic_write(&path, &bytes, policy)?;
    set_private_file(&path)
}

pub(super) fn clear_root(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            bail!("dedupe root contains a symlink");
        }
        if file_type.is_dir() {
            remove_generation_dir(&path)?;
        } else if file_type.is_file() {
            fs::remove_file(&path)?;
        } else {
            bail!("dedupe root contains a special file");
        }
    }
    storage_durable::sync_parent_dir(root)
}

pub(super) fn remove_generation_dir(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            bail!(
                "dedupe generation {} is not a real directory",
                path.display()
            )
        }
        Ok(_) => fs::remove_dir_all(path)
            .with_context(|| format!("remove expired dedupe generation {}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    storage_durable::sync_parent_dir(path)
}

pub(super) fn append_entries(path: &Path, entries: &[([u8; 32], u64, i64)]) -> Result<()> {
    let created = !path.exists();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("append dedupe index shard {}", path.display()))?;
    if created {
        set_private_file(path)?;
    }
    let mut bytes = Vec::with_capacity(entries.len() * ENTRY_BYTES);
    for (digest, cursor, acknowledged_at) in entries {
        bytes.extend_from_slice(digest);
        bytes.extend_from_slice(&cursor.to_le_bytes());
        bytes.extend_from_slice(&acknowledged_at.to_le_bytes());
    }
    file.write_all(&bytes)?;
    // The generation receipt WAL is the single durable batch boundary. Shard
    // files are a rebuildable lookup projection and intentionally do not fsync
    // on the ingest acknowledgement path.
    Ok(())
}

pub(super) fn seal_generation(directory: &Path) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    let mut paths = generation_shard_paths(directory)?;
    paths.sort();
    for path in paths {
        let mut entries = read_entries(&path)?;
        entries.sort_unstable_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.2.cmp(&right.2))
                .then(left.1.cmp(&right.1))
        });
        write_entries_with_policy(&path, &entries, storage_durable::FsyncPolicy::Os)?;
    }
    Ok(())
}

pub(super) fn generation_shard_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("dedupe generation contains a special file");
        }
        if entry.file_name() == RECEIPT_LOG_FILE {
            set_private_file(&path)?;
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("idx") {
            bail!(
                "dedupe generation contains an unknown file {}",
                path.display()
            );
        }
        if metadata.len() % ENTRY_BYTES as u64 != 0 {
            bail!("dedupe index shard {} has a torn record", path.display());
        }
        set_private_file(&path)?;
        paths.push(path);
    }
    paths.sort();
    Ok(paths)
}
