//! Keeping the dedupe index in step with the journal, and rebuilding it.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};

use crate::journal::domain::journal_limits::RECOVERY_PAGE_EVENTS;
use crate::journal::infrastructure::canonical_recovery_reader::CanonicalRecoveryReader;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::shared_kernel::stored_event::StoredEvent;

pub(super) fn rebuild_dedupe_index(
    root: &Path,
    storage: &crate::journal::infrastructure::storage::raw_storage::RawStorage,
    wal: &crate::storage::SignalWal,
    dedupe: &crate::storage::DedupeIndex,
    archived: crate::shared_kernel::archive_watermarks::ArchiveWatermarks,
    expected_last_cursor: u64,
) -> Result<()> {
    dedupe.reset()?;
    let mut page = Vec::with_capacity(RECOVERY_PAGE_EVENTS);
    let now = Utc::now();

    let cutoff = now - Duration::seconds(crate::storage::IDEMPOTENCY_WINDOW_SECONDS);
    let remote =
        crate::archive::application::replay_recent_committed::replay_recent_committed_events(
            root,
            cutoff,
            |event| {
                page.push(event);
                if page.len() == RECOVERY_PAGE_EVENTS {
                    append_unique_dedupe_page_at(dedupe, &mut page, now)?;
                    dedupe.maintain_at(now, false)?;
                }
                Ok(())
            },
        )?;
    if !page.is_empty() {
        append_unique_dedupe_page_at(dedupe, &mut page, now)?;
        dedupe.maintain_at(now, false)?;
    }

    let mut receipt_page =
        Vec::<crate::storage::DedupeReceipt>::with_capacity(RECOVERY_PAGE_EVENTS);
    crate::archive::application::replay_recent_committed::replay_recent_committed_receipts(
        root,
        cutoff,
        |receipt| {
            receipt_page.push(receipt);
            if receipt_page.len() == RECOVERY_PAGE_EVENTS {
                dedupe.append_receipts_at(&receipt_page, expected_last_cursor, now)?;
                receipt_page.clear();
                dedupe.maintain_at(now, false)?;
            }
            Ok(())
        },
    )?;
    if !receipt_page.is_empty() {
        dedupe.append_receipts_at(&receipt_page, expected_last_cursor, now)?;
        dedupe.maintain_at(now, false)?;
    }

    let remote_watermarks = remote
        .map(|_| archived)
        .unwrap_or_else(crate::shared_kernel::archive_watermarks::ArchiveWatermarks::default);
    let mut local_reader = CanonicalRecoveryReader::open(storage, wal, archived, 0, false)?;
    loop {
        let local = local_reader.read_page()?;
        if local.is_empty() {
            break;
        }
        let mut new_events = local
            .into_iter()
            .filter(|event| !remote_watermarks.covers(event.event.signal, event.cursor))
            .collect::<Vec<_>>();
        append_unique_dedupe_page_at(dedupe, &mut new_events, now)?;
        dedupe.maintain_at(now, false)?;
    }

    let stats = dedupe.stats_at(now)?;
    if stats.newest_cursor > expected_last_cursor {
        bail!(
            "rebuilt dedupe index cursor {} is ahead of journal cursor {expected_last_cursor}",
            stats.newest_cursor
        );
    }
    dedupe.mark_rebuilt_through(expected_last_cursor)?;
    dedupe.maintain_at(now, true)?;
    Ok(())
}

pub(super) fn append_unique_dedupe_page(
    dedupe: &crate::storage::DedupeIndex,
    page: &mut Vec<StoredEvent>,
) -> Result<()> {
    append_unique_dedupe_page_at(dedupe, page, Utc::now())
}

fn append_unique_dedupe_page_at(
    dedupe: &crate::storage::DedupeIndex,
    page: &mut Vec<StoredEvent>,
    now: DateTime<Utc>,
) -> Result<()> {
    if page.is_empty() {
        return Ok(());
    }
    let mut page_ids = HashMap::with_capacity(page.len());
    for stored in page.iter() {
        if !dedupe.covers(stored, now)? {
            continue;
        }
        if let Some(previous) = page_ids
            .insert(
                (stored.event.project.clone(), stored.event.event_id.clone()),
                stored.cursor,
            )
            .or(dedupe.lookup_at(&stored.event.project, &stored.event.event_id, now)?)
        {
            bail!(
                "journal contains duplicate event_id {} at cursors {previous} and {}",
                stored.event.event_id,
                stored.cursor
            );
        }
    }
    dedupe.append_batch_at(page, now)?;
    page.clear();
    Ok(())
}

impl DurableJournal {
    pub(crate) fn maintain_dedupe_at(&self, _now: DateTime<Utc>, force: bool) -> Result<usize> {
        match self.dedupe.maintain_applied(force) {
            Ok(flushed) => Ok(flushed),
            Err(maintenance_error) => {
                let archived =
                    crate::archive::application::archive_status_queries::committed_watermarks(
                        self.data_dir(),
                    )?;
                let expected_last_cursor = self.last_cursor();
                rebuild_dedupe_index(
                    self.data_dir(),
                    &self.storage,
                    &self.wal,
                    &self.dedupe,
                    archived,
                    expected_last_cursor,
                )
                .with_context(|| {
                    format!(
                        "dedupe projection maintenance failed ({maintenance_error:#}); canonical rebuild failed"
                    )
                })?;
                self.dedupe.maintain_applied(force)
            }
        }
    }
}
