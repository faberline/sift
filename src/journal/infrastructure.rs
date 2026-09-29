//! Where the journal lives: the durable journal over raw storage, its recovery
//! reader, the Raft state machine and its snapshots and checkpoints, and the
//! on-disk WAL, segments, blobs and dedupe index.

pub(crate) mod archive_prefix_source;
pub(crate) mod blob_reference_scan;
pub(crate) mod canonical_recovery_reader;
pub(crate) mod durable_journal;
pub(crate) mod journal_projection_read_session;
pub(crate) mod raft;
pub(crate) mod snapshot_codec;
pub(crate) mod storage;
