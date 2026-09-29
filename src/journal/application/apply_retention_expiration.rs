//! Evicting expired resident events and applying an expiration head.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::journal::application::dedupe_window::rebuild_dedupe_index;
use crate::journal::domain::event_content_digest::xor_event_content_digest;
use crate::journal::domain::recent_cursor::recent_cursor_map;
use crate::journal::infrastructure::canonical_recovery_reader::CanonicalRecoveryReader;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::shared_kernel::event_content_digest::xor_digest;

impl DurableJournal {
    pub(crate) fn evict_resident_before(&self, cutoff: DateTime<Utc>) -> Result<usize> {
        let mut state = self.state.write().expect("journal state lock poisoned");
        let before = state.recent_events.len();
        let mut retained = VecDeque::with_capacity(before);
        while let Some(event) = state.recent_events.pop_front() {
            let occurred = DateTime::parse_from_rfc3339(&event.event.occurred_at)
                .context("resident event occurred_at must be RFC3339")?
                .with_timezone(&Utc);
            if occurred >= cutoff {
                retained.push_back(event);
            }
        }
        state.recent_events = retained;
        state.recent_cursors_by_event_id = recent_cursor_map(&state.recent_events)?;
        Ok(before.saturating_sub(state.recent_events.len()))
    }

    pub(crate) fn apply_expiration_head(
        &self,
        cutoff: DateTime<Utc>,
        archived_prefix_events: u64,
        retained_prefix_events: u64,
        archived_prefix_digest: [u8; 32],
        retained_prefix_digest: [u8; 32],
        repair_dedupe: bool,
    ) -> Result<()> {
        let retention_status =
            crate::archive::application::archive_status_queries::committed_status(self.data_dir())?
                .context("expiration requires a committed archive")?;
        let retention_generation = retention_status.retention_generation;
        let archived = crate::archive::application::archive_status_queries::committed_watermarks(
            self.data_dir(),
        )?;
        let mut state = self.state.write().expect("journal state lock poisoned");
        if !repair_dedupe && state.total_events < archived_prefix_events {
            bail!("journal contains fewer events than its prior archive prefix");
        }
        if retained_prefix_events > archived_prefix_events {
            bail!("retention cannot add events to an archived prefix");
        }
        let suffix_events = state
            .last_cursor
            .checked_sub(archived.max_cursor())
            .context("archive cursor is ahead of the journal head")?;
        let retained_events = retained_prefix_events
            .checked_add(suffix_events)
            .context("retained event count exhausted u64")?;
        if retained_events > state.last_cursor {
            bail!("retained event count exceeds the journal cursor high-water mark");
        }

        let mut retained_resident = VecDeque::with_capacity(state.recent_events.len());
        while let Some(event) = state.recent_events.pop_front() {
            let occurred = DateTime::parse_from_rfc3339(&event.event.occurred_at)
                .context("resident event occurred_at must be RFC3339")?
                .with_timezone(&Utc);
            if event.cursor > archived.max_cursor() || occurred >= cutoff {
                retained_resident.push_back(event);
            }
        }
        state.recent_events = retained_resident;
        state.recent_cursors_by_event_id = recent_cursor_map(&state.recent_events)?;
        if !retention_status.retention_scan_pending
            && state.retention_generation != retention_generation
        {
            state.projection_generation = state.projection_generation.saturating_add(1);
        }
        state.total_events = retained_events;
        let suffix_digest = if repair_dedupe {
            let mut digest = [0_u8; 32];
            let mut recovery = CanonicalRecoveryReader::open(
                &self.storage,
                &self.wal,
                archived,
                archived.max_cursor(),
                true,
            )?;
            loop {
                let page = recovery.read_page()?;
                if page.is_empty() {
                    break;
                }
                for event in page {
                    xor_event_content_digest(&mut digest, &event.event)?;
                }
            }
            digest
        } else {
            xor_digest(state.event_content_digest, archived_prefix_digest)
        };
        state.event_content_digest = xor_digest(retained_prefix_digest, suffix_digest);
        state.retention_generation = retention_generation;
        let last_cursor = state.last_cursor;
        crate::storage::JournalHead::new(last_cursor, retained_events)
            .with_projection_generation(state.projection_generation)
            .with_retention_generation(retention_generation)
            .persist(self.data_dir())?;
        if repair_dedupe {
            rebuild_dedupe_index(
                self.data_dir(),
                &self.storage,
                &self.wal,
                &self.dedupe,
                archived,
                last_cursor,
            )?;
        }
        drop(state);
        self.recovery_required.store(false, Ordering::Release);
        Ok(())
    }
}
