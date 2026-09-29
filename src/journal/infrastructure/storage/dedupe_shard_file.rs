//! The dedupe receipt log and shard index files: encoding, reading, rebuilding
//! and looking up entries, with private file modes.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::journal::domain::bloom_filter::bloom_insert_scalable;
use crate::journal::domain::event_digest::{dedupe_record_digest, shard_for, xor_digest_in_place};
use crate::journal::infrastructure::storage::dedupe_generation_store::{
    append_entries, generation_shard_paths, parse_generation, seal_generation, GenerationState,
};
use crate::journal::infrastructure::storage::dedupe_index::DedupeRecord;

pub(super) const ENTRY_BYTES: usize = 48;

pub(super) const RECEIPT_LOG_FILE: &str = "receipts.framed";

pub(super) fn encode_receipt_records(entries: &[DedupeRecord]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(entries.len() * ENTRY_BYTES);
    for (digest, cursor, acknowledged_at) in entries {
        bytes.extend_from_slice(digest);
        bytes.extend_from_slice(&cursor.to_le_bytes());
        bytes.extend_from_slice(&acknowledged_at.to_le_bytes());
    }
    bytes
}

fn decode_receipt_records(bytes: &[u8]) -> Result<Vec<DedupeRecord>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(ENTRY_BYTES) {
        bail!("dedupe receipt frame has an invalid record boundary");
    }
    Ok(bytes
        .chunks_exact(ENTRY_BYTES)
        .map(|entry| {
            (
                entry[..32].try_into().expect("fixed digest bytes"),
                u64::from_le_bytes(entry[32..40].try_into().expect("fixed cursor bytes")),
                i64::from_le_bytes(
                    entry[40..48]
                        .try_into()
                        .expect("fixed acknowledgement bytes"),
                ),
            )
        })
        .collect())
}

pub(super) fn load_generation_receipts(directory: &Path, sealed: bool) -> Result<GenerationState> {
    let generation = parse_generation(
        directory
            .file_name()
            .and_then(|name| name.to_str())
            .context("dedupe generation path has no file name")?,
    )?;
    let receipt_path = directory.join(RECEIPT_LOG_FILE);
    if !receipt_path.exists() {
        bail!("dedupe generation is missing its receipt WAL");
    }
    let mut state = GenerationState::empty(sealed);
    storage_durable::FramedLogReader::visit_frames(&receipt_path, 0, |frame| {
        let records = decode_receipt_records(&frame.payload)?;
        let frame_cursor = records
            .iter()
            .map(|(_, cursor, _)| *cursor)
            .max()
            .context("dedupe receipt frame has no records")?;
        if frame.seq != frame_cursor {
            bail!("dedupe receipt frame sequence does not match its cursor");
        }
        for record @ (digest, cursor, acknowledged_at) in records {
            let shard = shard_for(&digest);
            bloom_insert_scalable(&mut state.blooms, &digest);
            state.entry_count = state.entry_count.saturating_add(1);
            state.newest_cursor = state.newest_cursor.max(cursor);
            state.newest_acknowledged_at_unix_nano = Some(
                state
                    .newest_acknowledged_at_unix_nano
                    .unwrap_or(i64::MIN)
                    .max(acknowledged_at),
            );
            xor_digest_in_place(
                &mut state.content_digest,
                dedupe_record_digest(generation, shard, &record),
            );
        }
        Ok(())
    })?;
    Ok(state)
}

pub(super) fn generation_shard_identity(directory: &Path) -> Result<(u64, [u8; 32])> {
    let generation = parse_generation(
        directory
            .file_name()
            .and_then(|name| name.to_str())
            .context("dedupe generation path has no file name")?,
    )?;
    let mut count = 0_u64;
    let mut digest = [0_u8; 32];
    for path in generation_shard_paths(directory)? {
        let shard = usize::from_str_radix(
            path.file_stem()
                .and_then(|name| name.to_str())
                .context("dedupe shard path has no file name")?,
            16,
        )
        .context("dedupe shard file has an invalid number")?;
        for record in read_entries(&path)? {
            count = count.saturating_add(1);
            xor_digest_in_place(
                &mut digest,
                dedupe_record_digest(generation, shard, &record),
            );
        }
    }
    Ok((count, digest))
}

pub(super) fn rebuild_generation_shards(directory: &Path, sealed: bool) -> Result<()> {
    for path in generation_shard_paths(directory)? {
        fs::remove_file(&path)
            .with_context(|| format!("remove rebuildable dedupe shard {}", path.display()))?;
    }
    storage_durable::sync_parent_dir(directory)?;
    let receipt_path = directory.join(RECEIPT_LOG_FILE);
    storage_durable::FramedLogReader::visit_frames(&receipt_path, 0, |frame| {
        let mut shards = BTreeMap::<usize, Vec<([u8; 32], u64, i64)>>::new();
        for record @ (digest, _, _) in decode_receipt_records(&frame.payload)? {
            shards.entry(shard_for(&digest)).or_default().push(record);
        }
        for (shard, records) in shards {
            append_entries(&directory.join(format!("{shard:03x}.idx")), &records)?;
        }
        Ok(())
    })?;
    if sealed {
        seal_generation(directory)?;
    }
    Ok(())
}

pub(super) fn lookup_file(
    path: &Path,
    digest: &[u8; 32],
    sorted: bool,
) -> Result<Option<(u64, i64)>> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = fs::metadata(path)?;
    if metadata.len() % ENTRY_BYTES as u64 != 0 {
        bail!("dedupe index shard {} has a torn record", path.display());
    }
    let entries = metadata.len() / ENTRY_BYTES as u64;
    let mut file = File::open(path)?;
    if !sorted {
        let mut entry = [0u8; ENTRY_BYTES];
        for _ in 0..entries {
            file.read_exact(&mut entry)?;
            if entry[..32] == digest[..] {
                return Ok(Some((
                    u64::from_le_bytes(entry[32..40].try_into().unwrap()),
                    i64::from_le_bytes(entry[40..48].try_into().unwrap()),
                )));
            }
        }
        return Ok(None);
    }
    let mut low = 0_u64;
    let mut high = entries;
    let mut entry = [0u8; ENTRY_BYTES];
    while low < high {
        let middle = low + (high - low) / 2;
        file.seek(SeekFrom::Start(middle * ENTRY_BYTES as u64))?;
        file.read_exact(&mut entry)?;
        match entry[..32].cmp(&digest[..]) {
            std::cmp::Ordering::Less => low = middle + 1,
            std::cmp::Ordering::Greater => high = middle,
            std::cmp::Ordering::Equal => {
                return Ok(Some((
                    u64::from_le_bytes(entry[32..40].try_into().unwrap()),
                    i64::from_le_bytes(entry[40..48].try_into().unwrap()),
                )))
            }
        }
    }
    Ok(None)
}

pub(super) fn read_entries(path: &Path) -> Result<Vec<DedupeRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(path)?;
    if bytes.len() % ENTRY_BYTES != 0 {
        bail!("dedupe index shard {} has a torn record", path.display());
    }
    Ok(bytes
        .chunks_exact(ENTRY_BYTES)
        .map(|entry| {
            (
                entry[..32].try_into().expect("fixed digest bytes"),
                u64::from_le_bytes(entry[32..40].try_into().expect("fixed cursor bytes")),
                i64::from_le_bytes(
                    entry[40..48]
                        .try_into()
                        .expect("fixed acknowledgement bytes"),
                ),
            )
        })
        .collect())
}

pub(super) fn write_entries_with_policy(
    path: &Path,
    entries: &[DedupeRecord],
    policy: storage_durable::FsyncPolicy,
) -> Result<()> {
    let mut bytes = Vec::with_capacity(entries.len() * ENTRY_BYTES);
    for (digest, cursor, acknowledged_at) in entries {
        bytes.extend_from_slice(digest);
        bytes.extend_from_slice(&cursor.to_le_bytes());
        bytes.extend_from_slice(&acknowledged_at.to_le_bytes());
    }
    storage_durable::atomic_write(path, &bytes, policy)?;
    set_private_file(path)
}

pub(super) fn set_private_dir(path: &Path) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub(super) fn set_private_file(path: &Path) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}
