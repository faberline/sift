//! Collecting local blobs the committed archive no longer references, in
//! batches.

use anyhow::{bail, Context, Result};

use crate::archive::application::archive_status_queries::committed_status;
use crate::archive::infrastructure::archive_catalog::catalog_for_uri;
use crate::archive::infrastructure::archive_control_paths::LOCAL_BLOB_GC_PATH;
use crate::archive::infrastructure::gcs_uri::blob_hash_from_archive_uri;
use crate::archive::infrastructure::local_blob_gc_store::{
    complete_local_blob_gc_plan, ensure_local_blob_gc_pending, persist_local_blob_gc_pending,
    read_local_blob_gc_pending,
};
use crate::archive::infrastructure::local_blob_live_index::{
    local_blob_is_live, mark_local_blob_live,
};

#[doc(hidden)]
pub fn resume_local_blob_gc_batch(
    journal: &crate::journal::infrastructure::durable_journal::DurableJournal,
    max_plan_entries: usize,
    max_scan_events: usize,
) -> Result<(usize, bool)> {
    if max_plan_entries == 0 || max_scan_events == 0 {
        bail!("local blob GC limits must be greater than zero");
    }
    ensure_local_blob_gc_pending(journal.data_dir())?;
    let path = journal.data_dir().join(LOCAL_BLOB_GC_PATH);
    if !path.exists() {
        return Ok((0, true));
    }
    let mut pending = read_local_blob_gc_pending(&path)?;
    committed_status(journal.data_dir())?
        .context("local blob GC requires a committed archive receipt")?;

    let (references, scanned_through, _scan_exhausted) =
        journal.scan_blob_references_page(pending.scanned_through_cursor, max_scan_events)?;
    for hash in references {
        mark_local_blob_live(
            journal.data_dir(),
            &pending.replacement_manifest_sha256,
            &hash,
        )?;
    }
    pending.scanned_through_cursor = scanned_through;
    persist_local_blob_gc_pending(&path, &pending)?;

    if pending.candidates.is_empty() && !pending.plan_exhausted {
        let catalog = catalog_for_uri(&pending.gc_plan_uri)?;
        let mut reader = match pending.plan_cursor.as_deref() {
            Some(cursor) => catalog.reader_after(&pending.gc_plan_root, cursor)?,
            None => catalog.reader(&pending.gc_plan_root)?,
        };
        let mut inspected = 0_usize;
        while inspected < max_plan_entries {
            let Some(entry) = reader.next() else {
                pending.plan_exhausted = true;
                break;
            };
            let entry = entry?;
            pending.plan_cursor = Some(entry.key);
            inspected += 1;
            let uri = String::from_utf8(entry.value).context("decode local blob GC object URI")?;
            if let Some(hash) = blob_hash_from_archive_uri(&uri) {
                pending.candidates.push(hash);
            }
        }
        pending.candidates.sort();
        pending.candidates.dedup();
        if pending.candidates.is_empty() {
            if pending.plan_exhausted {
                complete_local_blob_gc_plan(journal.data_dir(), &path, &pending)?;
                ensure_local_blob_gc_pending(journal.data_dir())?;
                return Ok((0, !path.exists()));
            }
            persist_local_blob_gc_pending(&path, &pending)?;
            return Ok((0, false));
        }
        persist_local_blob_gc_pending(&path, &pending)?;
    }

    let (scanned, removed, complete_batch) = journal.finalize_blob_candidates_with_index(
        &pending.candidates,
        pending.scanned_through_cursor,
        10_000,
        |hash| {
            mark_local_blob_live(
                journal.data_dir(),
                &pending.replacement_manifest_sha256,
                hash,
            )
        },
        |hash| {
            local_blob_is_live(
                journal.data_dir(),
                &pending.replacement_manifest_sha256,
                hash,
            )
        },
    )?;
    pending.scanned_through_cursor = scanned;
    if complete_batch {
        pending.candidates.clear();
        if pending.plan_exhausted {
            complete_local_blob_gc_plan(journal.data_dir(), &path, &pending)?;
            ensure_local_blob_gc_pending(journal.data_dir())?;
            return Ok((removed, !path.exists()));
        }
    }
    persist_local_blob_gc_pending(&path, &pending)?;
    Ok((removed, false))
}

pub(crate) fn finish_local_blob_gc(
    journal: &crate::journal::infrastructure::durable_journal::DurableJournal,
) -> Result<usize> {
    let mut removed = 0_usize;
    loop {
        let (batch_removed, complete) = resume_local_blob_gc_batch(journal, 128, 1_280_000)?;
        removed = removed.saturating_add(batch_removed);
        if complete {
            return Ok(removed);
        }
    }
}
