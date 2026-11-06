//! The dedupe index's counters.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DedupeStats {
    pub entry_count: u64,
    pub newest_cursor: u64,
    pub indexed_through_cursor: u64,
    pub oldest_generation: i64,
    pub newest_generation: i64,
    pub window_seconds: u64,
    pub rebuild_required: bool,
}
