//! Opening the journal: recovering the WAL and segments into the resident
//! window.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, RwLock};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use metrics_prometheus::Counter;

use crate::ingest::domain::governance_policy::GovernancePolicySet;
use crate::journal::application::dedupe_window::rebuild_dedupe_index;
use crate::journal::domain::event_content_digest::xor_event_content_digest;
use crate::journal::domain::journal_state::JournalState;
use crate::journal::domain::recent_cursor::{recent_cursor_at, RecentCursor};
use crate::journal::infrastructure::canonical_recovery_reader::CanonicalRecoveryReader;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::shared_kernel::event_content_digest::xor_digest;
use crate::shared_kernel::stored_event::StoredEvent;

impl DurableJournal {
    pub(in crate::journal) fn open_configured(
        data_dir: impl AsRef<Path>,
        governance: GovernancePolicySet,
        role: crate::storage::StorageRole,
        resident_limit: usize,
    ) -> Result<Self> {
        governance.validate()?;
        if resident_limit == 0 {
            bail!("resident journal event limit must be greater than zero");
        }
        let layout = crate::storage::DataLayout::open(data_dir, role)?;
        let data_dir = layout.root().to_path_buf();
        crate::archive::infrastructure::spill_catalog::cleanup_orphan_spills(&data_dir)?;
        crate::archive::infrastructure::archive_gc_pending_store::reconcile_staged_archive_gc(
            &data_dir,
        )?;
        let wal = crate::storage::SignalWal::open(&data_dir)?;
        let storage =
            crate::journal::infrastructure::storage::raw_storage::RawStorage::open(&data_dir)?;
        let stored_head = crate::storage::JournalHead::load(&data_dir)?;
        crate::archive::application::reconcile_committed_retention::reconcile_committed_retention(
            &data_dir,
            &storage,
            stored_head
                .as_ref()
                .map(|head| head.retention_generation)
                .unwrap_or_default(),
        )?;
        let archived =
            crate::archive::application::archive_status_queries::committed_watermarks(&data_dir)?;
        // A committed manifest is the durable authority for this prefix. Retry
        // compaction before comparing local segments with WAL bytes so a crash
        // after archive reconciliation cannot resurrect an older WAL copy.
        wal.compact_through(archived)?;
        let remote_retained =
            crate::archive::application::archive_status_queries::remote_retained_state(&data_dir)?;
        let mut state = JournalState::default();
        let (dedupe, dedupe_stats) = crate::storage::DedupeIndex::open(&data_dir)?;
        let recovery_time = Utc::now();
        let mut recovery = CanonicalRecoveryReader::open(&storage, &wal, archived, 0, true)?;
        let mut dedupe_matches = true;
        let mut local_after_remote = 0_u64;
        let mut local_after_remote_digest = [0_u8; 32];
        loop {
            let page = recovery.read_page()?;
            if page.is_empty() {
                break;
            }
            for stored in page {
                if remote_retained.is_none_or(|remote| {
                    !remote.watermarks.covers(stored.event.signal, stored.cursor)
                }) {
                    local_after_remote = local_after_remote.saturating_add(1);
                    xor_event_content_digest(&mut local_after_remote_digest, &stored.event)?;
                }
                if !dedupe_stats.rebuild_required
                    && dedupe.covers(&stored, recovery_time)?
                    && dedupe.lookup_at(
                        &stored.event.project,
                        &stored.event.event_id,
                        recovery_time,
                    )? != Some(stored.cursor)
                {
                    dedupe_matches = false;
                }
                Self::insert_recovered(&mut state, stored, resident_limit)?;
            }
        }

        let mut head = stored_head.unwrap_or_else(|| {
            crate::storage::JournalHead::new(
                state.last_cursor.max(archived.max_cursor()),
                state.total_events,
            )
        });
        head.last_cursor = head
            .last_cursor
            .max(state.last_cursor)
            .max(archived.max_cursor())
            .max(
                remote_retained
                    .map(|remote| remote.snapshot_index)
                    .unwrap_or_default(),
            );
        head.retained_events = match remote_retained {
            Some(remote) => remote
                .event_count
                .checked_add(local_after_remote)
                .context("retained event count exhausted u64")?,
            None => head.retained_events.max(state.total_events),
        };
        if let Some(remote) = remote_retained {
            if remote.retention_generation > head.retention_generation {
                if !remote.retention_scan_pending {
                    head.projection_generation = head.projection_generation.saturating_add(1);
                }
                head.retention_generation = remote.retention_generation;
            }
        }
        if head.retained_events > head.last_cursor {
            bail!("journal head retained event count exceeds the recovered cursor range");
        }

        if dedupe_stats.rebuild_required
            || dedupe_stats.indexed_through_cursor != head.last_cursor
            || dedupe_stats.newest_cursor > head.last_cursor
            || !dedupe_matches
        {
            rebuild_dedupe_index(
                &data_dir,
                &storage,
                &wal,
                &dedupe,
                archived,
                head.last_cursor,
            )?;
        }
        head.persist(&data_dir)?;
        // A durable archive receipt authorizes WAL truncation. Retry a crash
        // or late I/O failure before this journal starts serving requests.
        wal.compact_through(archived)?;
        state.last_cursor = head.last_cursor;
        state.total_events = head.retained_events;
        state.projection_generation = head.projection_generation;
        state.retention_generation = head.retention_generation;
        if let Some(remote) = remote_retained {
            state.event_content_digest =
                xor_digest(remote.event_content_sha256, local_after_remote_digest);
        }
        let accepted = head.retained_events;
        let journal = Self {
            _layout: layout,
            wal,
            storage,
            dedupe,
            blob_gate: Mutex::new(()),
            state: RwLock::new(state),
            resident_limit,
            governance,
            accepted: Counter::new(),
            duplicates: Counter::new(),
            fsyncs: Counter::new(),
            recovery_required: AtomicBool::new(false),
            retention_fenced: AtomicBool::new(false),
        };
        journal.accepted.add(accepted);
        if let Err(error) =
            crate::archive::application::resume_local_blob_gc::resume_local_blob_gc_batch(
                &journal, 128, 1_280_000,
            )
        {
            tracing::warn!(%error, "resume local blob GC after restart failed; durable progress is retained");
        }
        Ok(journal)
    }

    pub(super) fn insert_recovered(
        state: &mut JournalState,
        stored: StoredEvent,
        resident_limit: usize,
    ) -> Result<()> {
        stored.event.validate()?;
        let acknowledged_at = DateTime::parse_from_rfc3339(&stored.acknowledged_at)
            .context("recovered event acknowledged_at must be RFC3339")?
            .with_timezone(&Utc);
        if stored.cursor <= state.last_cursor {
            bail!(
                "journal cursor {} is not strictly after recovered cursor {}",
                stored.cursor,
                state.last_cursor
            );
        }
        if recent_cursor_at(
            &state.recent_cursors_by_event_id,
            &stored.event.project,
            &stored.event.event_id,
            acknowledged_at,
        )
        .is_some()
        {
            bail!(
                "journal contains duplicate event_id {}",
                stored.event.event_id
            );
        }
        Self::push_resident(state, stored, resident_limit)?;
        Ok(())
    }

    pub(super) fn push_resident(
        state: &mut JournalState,
        stored: StoredEvent,
        resident_limit: usize,
    ) -> Result<()> {
        xor_event_content_digest(&mut state.event_content_digest, &stored.event)
            .expect("validated Sift event must have a stable content digest");
        let recent = RecentCursor::from_stored(&stored)?;
        state.last_cursor = stored.cursor;
        state.total_events = state.total_events.saturating_add(1);
        state.recent_cursors_by_event_id.insert(
            (stored.event.project.clone(), stored.event.event_id.clone()),
            recent,
        );
        state.recent_events.push_back(stored);
        while state.recent_events.len() > resident_limit {
            if let Some(evicted) = state.recent_events.pop_front() {
                let remove = state
                    .recent_cursors_by_event_id
                    .get(&(
                        evicted.event.project.clone(),
                        evicted.event.event_id.clone(),
                    ))
                    .is_some_and(|recent| recent.cursor == evicted.cursor);
                if remove {
                    state.recent_cursors_by_event_id.remove(&(
                        evicted.event.project.clone(),
                        evicted.event.event_id.clone(),
                    ));
                }
            }
        }
        Ok(())
    }
}
