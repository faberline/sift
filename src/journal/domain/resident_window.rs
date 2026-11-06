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
}
