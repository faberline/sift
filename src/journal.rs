//! The journal: Sift's durable, replicated event log. It accepts governed event
//! batches through Raft, deduplicates them inside the idempotency window, keeps
//! a resident window of recent events, recovers from the WAL and sealed
//! segments, adopts archive checkpoints and retention, and serves reads to the
//! projections and queries.

pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;
