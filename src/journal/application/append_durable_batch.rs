//! Appending a durable, deduplicated batch of governed events.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::event::EventEnvelope;
use crate::journal::application::dedupe_window::rebuild_dedupe_index;
use crate::journal::domain::append_result::AppendResult;
use crate::journal::domain::recent_cursor::recent_receipt_at;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::shared_kernel::retention_boundary::retention_rejection_at;
use crate::shared_kernel::stored_event::StoredEvent;

impl DurableJournal {
    pub fn append(&self, event: EventEnvelope) -> Result<AppendResult> {
        self.append_durable_batch(vec![event])?
            .pop()
            .context("single-event durable batch returned no result")
    }

    /// Apply one committed single-signal batch to the canonical WAL.
    ///
    /// The batch is encoded as one WAL frame and reaches one fsync boundary.
    /// Segment writes are rebuildable work and do not participate in the
    /// acknowledgement boundary.
    pub(crate) fn append_durable_batch(
        &self,
        events: Vec<EventEnvelope>,
    ) -> Result<Vec<AppendResult>> {
        self.append_durable_batch_at(events, Utc::now())
    }

    pub(crate) fn append_durable_batch_at(
        &self,
        events: Vec<EventEnvelope>,
        acknowledged_at: DateTime<Utc>,
    ) -> Result<Vec<AppendResult>> {
        self.ensure_recovered()?;
        self.dedupe.advance_window_at(acknowledged_at)?;
        self.dedupe
            .preflight_append_at(acknowledged_at, events.len())?;
        // Blob externalization happens before the event reaches the WAL. Hold
        // this gate through the durable append so retention cannot delete a
        // newly created blob before its event reference becomes visible.
        let _blob_gate = self.blob_gate.lock().expect("Sift blob gate poisoned");
        let signal = events
            .first()
            .context("Sift durable batch must not be empty")?
            .signal;
        if events.iter().any(|event| event.signal != signal) {
            bail!("Sift durable batch must contain exactly one signal");
        }
        let mut governed = Vec::with_capacity(events.len());
        for event in events {
            let event = self.govern_event(event)?;
            event.validate()?;
            governed.push(event);
        }

        let mut state = self.state.write().expect("journal state lock poisoned");
        let mut next_cursor = state
            .last_cursor
            .checked_add(1)
            .context("Sift journal cursor exhausted u64")?;
        let mut staged = Vec::with_capacity(governed.len());
        let mut staged_cursors = HashMap::<(String, String), u64>::new();
        let mut results = Vec::with_capacity(governed.len());

        for mut event in governed {
            let duplicate_receipt = recent_receipt_at(
                &state.recent_cursors_by_event_id,
                &event.project,
                &event.event_id,
                acknowledged_at,
            )
            .map(|(cursor, accepted)| (cursor, accepted.to_rfc3339()))
            .or_else(|| {
                staged_cursors
                    .get(&(event.project.clone(), event.event_id.clone()))
                    .copied()
                    .map(|cursor| (cursor, acknowledged_at.to_rfc3339()))
            })
            .or(self
                .dedupe
                .lookup_record_at(&event.project, &event.event_id, acknowledged_at)?
                .map(|(cursor, accepted)| {
                    (
                        cursor,
                        DateTime::<Utc>::from_timestamp_nanos(accepted).to_rfc3339(),
                    )
                }));
            if let Some((cursor, original_acknowledged_at)) = duplicate_receipt {
                self.duplicates.incr();
                results.push(AppendResult {
                    event_id: event.event_id,
                    acknowledged_at: original_acknowledged_at,
                    cursor,
                    raw_cursor: cursor,
                    commit_index: cursor,
                    duplicate: true,
                });
                continue;
            }

            // A successful acknowledgement owns its exact six-hour retry
            // window even when the telemetry event crosses the 180-day
            // retention boundary meanwhile. Only a new event is subject to
            // retention admission.
            if let Some(message) = retention_rejection_at(&event, acknowledged_at) {
                bail!(message);
            }

            self.storage
                .externalize_event(&mut event)
                .context("durably externalize raw event payload")?;
            event.validate()?;
            let cursor = next_cursor;
            next_cursor = next_cursor
                .checked_add(1)
                .context("Sift journal cursor exhausted u64")?;
            let event_id = event.event_id.clone();
            staged_cursors.insert((event.project.clone(), event_id.clone()), cursor);
            staged.push(StoredEvent {
                cursor,
                acknowledged_at: acknowledged_at.to_rfc3339(),
                event,
            });
            results.push(AppendResult {
                event_id,
                acknowledged_at: acknowledged_at.to_rfc3339(),
                cursor,
                raw_cursor: cursor,
                commit_index: cursor,
                duplicate: false,
            });
        }

        if !staged.is_empty() {
            self.wal
                .append_batch(&staged)
                .context("append and fsync one signal WAL batch before acknowledgement")?;
            self.fsyncs.incr();
            if let Err(error) = self.storage.append_batch(&staged) {
                tracing::warn!(
                    %error,
                    first_cursor = staged.first().map(|event| event.cursor),
                    last_cursor = staged.last().map(|event| event.cursor),
                    "deferred segment append failed; canonical WAL remains recoverable"
                );
            }
            if let Err(append_error) = self.dedupe.append_batch_at(&staged, acknowledged_at) {
                let archived =
                    crate::archive::application::archive_status_queries::committed_watermarks(
                        self.data_dir(),
                    )?;
                let expected_last_cursor = staged
                    .last()
                    .map(|stored| stored.cursor)
                    .unwrap_or(state.last_cursor);
                if let Err(rebuild_error) = rebuild_dedupe_index(
                    self.data_dir(),
                    &self.storage,
                    &self.wal,
                    &self.dedupe,
                    archived,
                    expected_last_cursor,
                ) {
                    self.recovery_required.store(true, Ordering::Release);
                    bail!(
                        "dedupe index append failed ({append_error:#}); rebuild failed ({rebuild_error:#})"
                    );
                }
            }
            for stored in staged {
                Self::push_resident(&mut state, stored, self.resident_limit)?;
            }
            crate::storage::JournalHead::new(state.last_cursor, state.total_events)
                .with_projection_generation(state.projection_generation)
                .with_retention_generation(state.retention_generation)
                .persist(self.data_dir())
                .context("persist journal head before acknowledging the durable batch")?;
            self.accepted.add(staged_cursors.len() as u64);
        }
        Ok(results)
    }
}
