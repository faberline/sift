//! Answering event queries, projection reads and replays from the journal.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};

use crate::journal::domain::append_result::AppendResult;
use crate::journal::domain::event_query::EventQuery;
use crate::journal::domain::recent_cursor::recent_receipt_at;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::shared_kernel::stored_event::StoredEvent;

impl DurableJournal {
    pub(super) fn result_for_at(
        &self,
        project: &str,
        event_id: &str,
        decision_time: DateTime<Utc>,
    ) -> Result<Option<AppendResult>> {
        let recent = {
            let state = self.state.read().expect("journal state lock poisoned");
            recent_receipt_at(
                &state.recent_cursors_by_event_id,
                project,
                event_id,
                decision_time,
            )
            .map(|(cursor, acknowledged_at)| (cursor, acknowledged_at.to_rfc3339()))
        };
        let receipt = match recent {
            Some(receipt) => Some(receipt),
            None => self
                .dedupe
                .lookup_record_at(project, event_id, decision_time)?
                .map(|(cursor, acknowledged_at)| {
                    (
                        cursor,
                        DateTime::<Utc>::from_timestamp_nanos(acknowledged_at).to_rfc3339(),
                    )
                }),
        };
        Ok(receipt.map(|(cursor, acknowledged_at)| AppendResult {
            event_id: event_id.to_string(),
            acknowledged_at,
            cursor,
            raw_cursor: cursor,
            commit_index: cursor,
            duplicate: true,
        }))
    }

    pub fn query(&self, query: EventQuery) -> Result<Vec<StoredEvent>> {
        self.ensure_queryable()?;
        self.query_unchecked(query)
    }

    pub(in crate::journal) fn query_unchecked(
        &self,
        query: EventQuery,
    ) -> Result<Vec<StoredEvent>> {
        self.query_local_unchecked(query, 10_000)
    }

    pub(in crate::journal) fn query_local_unchecked(
        &self,
        query: EventQuery,
        maximum_limit: usize,
    ) -> Result<Vec<StoredEvent>> {
        let limit = if query.limit == 0 {
            100
        } else {
            query.limit.clamp(1, maximum_limit.max(1))
        };
        let state = self.state.read().expect("journal state lock poisoned");
        let mut by_cursor = BTreeMap::<u64, StoredEvent>::new();
        for event in self
            .storage
            .query_events(query.signal, query.after, limit)?
        {
            by_cursor.insert(event.cursor, event);
        }
        for event in state
            .recent_events
            .iter()
            .filter(|entry| entry.cursor > query.after)
            .filter(|entry| {
                query
                    .signal
                    .is_none_or(|signal| entry.event.signal == signal)
            })
        {
            if let Some(existing) = by_cursor.insert(event.cursor, event.clone()) {
                if existing.event.event_id != event.event.event_id {
                    bail!(
                        "disk and resident journal disagree at cursor {}",
                        event.cursor
                    );
                }
            }
        }
        Ok(by_cursor.into_values().take(limit).collect())
    }

    pub(crate) fn query_projection_events(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<Vec<StoredEvent>> {
        self.ensure_recovered()?;
        let limit = limit.clamp(1, 10_000);
        let mut by_cursor = BTreeMap::<u64, StoredEvent>::new();
        let archive_status =
            crate::archive::application::archive_status_queries::committed_status(self.data_dir())?;
        if archive_status
            .as_ref()
            .is_some_and(|status| after < status.snapshot_index)
        {
            if let Some(events) =
                crate::archive::application::replay_committed_events::read_committed_events_after(
                    self.data_dir(),
                    after,
                    limit,
                )?
            {
                for event in events {
                    by_cursor.insert(event.cursor, event);
                }
            }
        }
        if by_cursor.len() < limit {
            for event in self.query_unchecked(EventQuery {
                signal: None,
                after,
                limit,
            })? {
                by_cursor.insert(event.cursor, event);
            }
        }
        Ok(by_cursor.into_values().take(limit).collect())
    }

    pub fn replay(&self, after: u64, limit: usize) -> Result<Vec<StoredEvent>> {
        self.query(EventQuery {
            signal: None,
            after,
            limit,
        })
    }
}
