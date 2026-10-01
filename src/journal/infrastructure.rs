//! Where the journal lives: the durable journal over raw storage, its recovery
//! reader, the Raft state machine and its snapshots and checkpoints, and the
//! on-disk WAL, segments, blobs and dedupe index.

pub(crate) mod canonical_recovery_reader;
pub(crate) mod raft;
pub(crate) mod storage;

pub(crate) mod recovery_store_adapter;

pub(crate) mod snapshot_codec_adapter;
