//! Monotonic journal identity that survives hot-segment eviction.
//!
//! A cold event can live only in the committed archive. The local segment set
//! can therefore be empty while the next accepted event must still receive a
//! cursor larger than every earlier event. This small control record owns that
//! monotonic high-water mark and the retained event count.

use serde::{Deserialize, Serialize};

pub(in crate::journal) const JOURNAL_HEAD_FORMAT_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JournalHead {
    pub(in crate::journal) format_version: u16,
    pub last_cursor: u64,
    pub retained_events: u64,
    #[serde(default)]
    pub projection_generation: u64,
    #[serde(default)]
    pub retention_generation: u64,
}

impl JournalHead {
    pub fn new(last_cursor: u64, retained_events: u64) -> Self {
        Self {
            format_version: JOURNAL_HEAD_FORMAT_VERSION,
            last_cursor,
            retained_events,
            projection_generation: 0,
            retention_generation: 0,
        }
    }

    pub fn with_projection_generation(mut self, projection_generation: u64) -> Self {
        self.projection_generation = projection_generation;
        self
    }

    pub fn with_retention_generation(mut self, retention_generation: u64) -> Self {
        self.retention_generation = retention_generation;
        self
    }
}
