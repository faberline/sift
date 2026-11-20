//! The resident window: the journal's in-memory tail of recent stored events,
//! and the counters kept with it.

use std::collections::{HashMap, VecDeque};

use crate::journal::domain::recent_cursor::RecentCursor;
use crate::shared_kernel::stored_event::StoredEvent;

#[derive(Default)]
pub(in crate::journal) struct ResidentWindow {
    pub(in crate::journal) recent_events: VecDeque<StoredEvent>,
    pub(in crate::journal) recent_cursors_by_event_id: HashMap<(String, String), RecentCursor>,
    pub(in crate::journal) last_cursor: u64,
    pub(in crate::journal) total_events: u64,
    pub(in crate::journal) projection_generation: u64,
    pub(in crate::journal) retention_generation: u64,
    pub(in crate::journal) event_content_digest: [u8; 32],
    pub(in crate::journal) resident_bytes: usize,
}

/// The JSON size of one resident event: the size the byte limit counts.
pub(in crate::journal) fn resident_event_bytes(stored: &StoredEvent) -> usize {
    serde_json::to_vec(stored)
        .map(|bytes| bytes.len())
        .unwrap_or(0)
}

impl ResidentWindow {
    /// Recompute `resident_bytes` after `recent_events` was rebuilt.
    pub(in crate::journal) fn recompute_resident_bytes(&mut self) {
        self.resident_bytes = self.recent_events.iter().fold(0_usize, |total, stored| {
            total.saturating_add(resident_event_bytes(stored))
        });
    }
}
