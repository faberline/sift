//! Validating and restoring the journal from a snapshot stream.

use std::io::{Read, Seek, SeekFrom};

use anyhow::{bail, Context, Result};

use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::journal::infrastructure::raft::archive_checkpoint::{
    read_archive_checkpoint, restore_archive_checkpoint, ARCHIVE_CHECKPOINT_MAGIC,
};
use crate::journal::infrastructure::raft::archive_checkpoint_install::{
    validate_archive_checkpoint_install, ValidatedArchiveStage,
};
use crate::journal::infrastructure::raft::local_checkpoint::{
    read_local_checkpoint, restore_local_checkpoint, LOCAL_CHECKPOINT_MAGIC,
};
use crate::journal::infrastructure::raft::resident_checkpoint::{
    read_resident_checkpoint, restore_resident_checkpoint, RESIDENT_CHECKPOINT_MAGIC,
};
use crate::journal::infrastructure::raft::snapshot_format::{
    read_one_or_eof, read_snapshot_header, SnapshotMetadata, MAX_SNAPSHOT_EVENT_BYTES,
    SNAPSHOT_FRAME_HEADER_BYTES, SNAPSHOT_PAGE_EVENTS,
};
use crate::shared_kernel::stored_event::StoredEvent;

/// Restore a seekable snapshot only after a complete validation pass.
///
/// This prevents a corrupt or truncated snapshot from partially filling an
/// empty Sift data directory.
pub(crate) fn restore_seekable_snapshot<R>(
    journal: &DurableJournal,
    reader: &mut R,
) -> Result<SnapshotMetadata>
where
    R: Read + Seek,
{
    if journal.last_cursor() != 0 {
        bail!("snapshot restore requires an empty Sift data directory");
    }

    reader
        .seek(SeekFrom::Start(0))
        .context("seek to the start of the Sift snapshot")?;
    let validated = read_snapshot(reader, |_| Ok(()))?;

    reader
        .seek(SeekFrom::Start(0))
        .context("rewind the validated Sift snapshot")?;
    let mut page = Vec::with_capacity(SNAPSHOT_PAGE_EVENTS);
    let restored = read_snapshot(reader, |event| {
        page.push(event);
        if page.len() == SNAPSHOT_PAGE_EVENTS {
            journal.restore_stored_page(std::mem::take(&mut page))?;
            page.reserve(SNAPSHOT_PAGE_EVENTS);
        }
        Ok(())
    })?;
    if !page.is_empty() {
        journal.restore_stored_page(page)?;
    }
    if restored != validated {
        bail!("Sift snapshot metadata changed between validation and restore");
    }
    if journal.total_event_count() != restored.event_count
        || journal.last_cursor() != restored.last_cursor
    {
        bail!("restored Sift snapshot does not match its declared event count and cursor");
    }
    Ok(restored)
}

/// Spool a non-seekable Raft snapshot in the data directory, validate it, and
/// then restore it in bounded pages. The temporary file stays on the same file
/// system as all other Sift atomic work.
pub(super) fn restore_streamed_snapshot(
    journal: &DurableJournal,
    current_applied_index: u64,
    reader: &mut dyn Read,
    staged_archive: Option<ValidatedArchiveStage>,
) -> Result<SnapshotMetadata> {
    let tmp_dir = journal.data_dir().join("tmp");
    let mut spool = tempfile::tempfile_in(&tmp_dir)
        .with_context(|| format!("create snapshot spool in {}", tmp_dir.display()))?;
    std::io::copy(reader, &mut spool).context("spool incoming Sift snapshot")?;
    spool
        .sync_all()
        .context("sync incoming Sift snapshot spool")?;
    spool
        .seek(SeekFrom::Start(0))
        .context("rewind incoming Sift snapshot spool")?;
    let mut magic = [0_u8; 8];
    spool
        .read_exact(&mut magic)
        .context("read incoming Sift snapshot magic")?;
    spool
        .seek(SeekFrom::Start(0))
        .context("rewind incoming Sift snapshot after magic")?;
    if &magic == ARCHIVE_CHECKPOINT_MAGIC {
        let checkpoint = read_archive_checkpoint(&mut spool)?;
        return restore_archive_checkpoint(journal, &checkpoint, staged_archive);
    }
    if &magic == LOCAL_CHECKPOINT_MAGIC {
        let checkpoint = read_local_checkpoint(&mut spool)?;
        return restore_local_checkpoint(journal, &checkpoint);
    }
    if &magic == RESIDENT_CHECKPOINT_MAGIC {
        let checkpoint = read_resident_checkpoint(&mut spool)?;
        return restore_resident_checkpoint(journal, &checkpoint);
    }

    let metadata = read_snapshot_header(&mut spool)?;
    if metadata.applied_index <= current_applied_index {
        return Ok(metadata);
    }
    spool
        .seek(SeekFrom::Start(0))
        .context("rewind incoming full Sift snapshot")?;
    restore_seekable_snapshot(journal, &mut spool)
}

/// Validate all snapshot bytes and remote objects without changing the live
/// journal. A full archive restore is staged in `tmp` so the later durable
/// Raft install never discovers a bad manifest or segment after compaction.
pub(super) fn validate_streamed_snapshot(
    journal: &DurableJournal,
    reader: &mut dyn Read,
) -> Result<Option<ValidatedArchiveStage>> {
    let tmp_dir = journal.data_dir().join("tmp");
    let mut spool = tempfile::tempfile_in(&tmp_dir)
        .with_context(|| format!("create snapshot validation spool in {}", tmp_dir.display()))?;
    std::io::copy(reader, &mut spool).context("spool incoming Sift snapshot for validation")?;
    spool
        .seek(SeekFrom::Start(0))
        .context("rewind incoming Sift snapshot validation spool")?;
    let mut magic = [0_u8; 8];
    spool
        .read_exact(&mut magic)
        .context("read incoming Sift snapshot validation magic")?;
    spool
        .seek(SeekFrom::Start(0))
        .context("rewind incoming Sift snapshot validation spool after magic")?;
    if &magic == ARCHIVE_CHECKPOINT_MAGIC {
        let checkpoint = read_archive_checkpoint(&mut spool)?;
        return validate_archive_checkpoint_install(journal, &checkpoint);
    }
    if &magic == LOCAL_CHECKPOINT_MAGIC {
        let checkpoint = read_local_checkpoint(&mut spool)?;
        restore_local_checkpoint(journal, &checkpoint)?;
        return Ok(None);
    }
    if &magic == RESIDENT_CHECKPOINT_MAGIC {
        let checkpoint = read_resident_checkpoint(&mut spool)?;
        restore_resident_checkpoint(journal, &checkpoint)?;
        return Ok(None);
    }
    read_snapshot(&mut spool, |_| Ok(())).map(|_| None)
}

fn read_snapshot<R, F>(reader: &mut R, mut on_event: F) -> Result<SnapshotMetadata>
where
    R: Read,
    F: FnMut(StoredEvent) -> Result<()>,
{
    let metadata = read_snapshot_header(reader)?;
    let mut expected_cursor = 1_u64;
    let mut last_cursor = 0_u64;
    for position in 0..metadata.event_count {
        let mut frame = [0_u8; SNAPSHOT_FRAME_HEADER_BYTES];
        reader
            .read_exact(&mut frame)
            .with_context(|| format!("read Sift snapshot frame {position}"))?;
        let cursor = u64::from_le_bytes(frame[0..8].try_into().unwrap());
        let payload_len = u32::from_le_bytes(frame[8..12].try_into().unwrap()) as usize;
        let expected_checksum = u32::from_le_bytes(frame[12..16].try_into().unwrap());
        if payload_len == 0 || payload_len > MAX_SNAPSHOT_EVENT_BYTES {
            bail!("Sift snapshot event at cursor {cursor} has invalid length {payload_len}");
        }
        let mut payload = vec![0_u8; payload_len];
        reader
            .read_exact(&mut payload)
            .with_context(|| format!("read Sift snapshot event payload at cursor {cursor}"))?;
        let actual_checksum = crc32fast::hash(&payload);
        if actual_checksum != expected_checksum {
            bail!("Sift snapshot checksum mismatch at cursor {cursor}");
        }
        let stored: StoredEvent = serde_json::from_slice(&payload)
            .with_context(|| format!("decode Sift snapshot event at cursor {cursor}"))?;
        if cursor != stored.cursor {
            bail!(
                "Sift snapshot frame cursor {cursor} does not match payload cursor {}",
                stored.cursor
            );
        }
        if cursor != expected_cursor {
            bail!("Sift snapshot cursor {cursor} is out of order; expected {expected_cursor}");
        }
        stored
            .event
            .validate()
            .with_context(|| format!("validate Sift snapshot event at cursor {cursor}"))?;
        on_event(stored)?;
        last_cursor = cursor;
        expected_cursor = expected_cursor
            .checked_add(1)
            .context("snapshot cursor exhausted u64")?;
    }
    if last_cursor != metadata.last_cursor {
        bail!(
            "Sift snapshot ended at cursor {last_cursor}, expected {}",
            metadata.last_cursor
        );
    }
    if metadata.event_count == 0 && metadata.last_cursor != 0 {
        bail!("empty Sift snapshot declares a non-zero last cursor");
    }
    if read_one_or_eof(reader)?.is_some() {
        bail!("Sift snapshot contains trailing bytes");
    }
    Ok(metadata)
}
