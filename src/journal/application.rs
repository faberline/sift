//! What the journal does: recovering its resident window, appending governed
//! batches through Raft, keeping the dedupe window, adopting archive
//! checkpoints, retention and restores, and answering event queries.

pub(crate) mod adopt_archive;
pub(crate) mod append_durable_batch;
pub(crate) mod apply_retention_expiration;
pub(crate) mod checkpoint_identity;
pub(crate) mod commit_append_batch;
pub(crate) mod dedupe_window;
pub(crate) mod query_events;
pub(crate) mod recover_journal;
pub(crate) mod restore_journal;
