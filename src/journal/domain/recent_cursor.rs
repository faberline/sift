//! The recent cursors by event id, and whether one is still inside the
//! idempotency window.

use std::collections::{HashMap, VecDeque};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};

use crate::shared_kernel::stored_event::StoredEvent;

#[derive(Clone, Debug)]
pub(in crate::journal) struct RecentCursor {
    pub(in crate::journal) cursor: u64,
    pub(in crate::journal) acknowledged_at: DateTime<Utc>,
}

impl RecentCursor {
    pub(in crate::journal) fn from_stored(event: &StoredEvent) -> Result<Self> {
        Ok(Self {
            cursor: event.cursor,
            acknowledged_at: DateTime::parse_from_rfc3339(&event.acknowledged_at)
                .context("stored event acknowledged_at must be RFC3339")?
                .with_timezone(&Utc),
        })
    }

    fn active_at(&self, now: DateTime<Utc>) -> bool {
        self.acknowledged_at >= now - Duration::seconds(crate::storage::IDEMPOTENCY_WINDOW_SECONDS)
    }
}

pub(in crate::journal) fn recent_cursor_at(
    cursors: &HashMap<(String, String), RecentCursor>,
    project: &str,
    event_id: &str,
    now: DateTime<Utc>,
) -> Option<u64> {
    recent_receipt_at(cursors, project, event_id, now).map(|(cursor, _)| cursor)
}

pub(in crate::journal) fn recent_receipt_at(
    cursors: &HashMap<(String, String), RecentCursor>,
    project: &str,
    event_id: &str,
    now: DateTime<Utc>,
) -> Option<(u64, DateTime<Utc>)> {
    cursors
        .get(&(project.to_owned(), event_id.to_owned()))
        .filter(|recent| recent.active_at(now))
        .map(|recent| (recent.cursor, recent.acknowledged_at))
}

pub(in crate::journal) fn recent_cursor_map(
    events: &VecDeque<StoredEvent>,
) -> Result<HashMap<(String, String), RecentCursor>> {
    events
        .iter()
        .map(|event| {
            Ok((
                (event.event.project.clone(), event.event.event_id.clone()),
                RecentCursor::from_stored(event)?,
            ))
        })
        .collect()
}
