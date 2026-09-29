//! The archive: uploads immutable Parquet segments and blobs before the commit
//! manifest, then restores only hash-verified objects. It commits remote and
//! local archives of the journal, evicts hot segments after 30 days, expires
//! events after 180 days, collects the garbage a retention pass leaves,
//! restores and bootstraps a volume, replays cold events for queries, and runs
//! the leader's lifecycle worker.

pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;
