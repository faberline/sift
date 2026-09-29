//! What appending one event returned: its cursor, commit index and duplicate
//! bit.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct AppendResult {
    pub event_id: String,
    /// Leader-selected Raft decision time. The exact six-hour window starts
    /// at this returned timestamp, not at client-side response receipt time.
    pub acknowledged_at: String,
    /// Compatibility alias for `raw_cursor`.
    pub cursor: u64,
    pub raw_cursor: u64,
    pub commit_index: u64,
    /// True when Sift found the event ID inside its six-hour exact window.
    pub duplicate: bool,
}

impl AppendResult {
    pub(in crate::journal) fn with_commit_index(mut self, commit_index: u64) -> Self {
        self.commit_index = commit_index;
        self
    }
}
