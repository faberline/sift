//! The journal's on-disk storage: the signal WAL, epoch shard routing, sealed
//! segment files, the content-addressed blob store, the journal head file and
//! the rebuildable dedupe index.

pub(crate) mod blob_store;
pub(crate) mod dedupe_append;
pub(crate) mod dedupe_generation_store;
pub(crate) mod dedupe_index;
pub(crate) mod dedupe_maintenance;
pub(crate) mod dedupe_replace;
pub(crate) mod dedupe_shard_file;
pub(crate) mod dedupe_tree_copy;
pub(crate) mod journal_head_file;
pub(crate) mod local_eviction;
pub(crate) mod raw_storage;
pub(crate) mod retained_prefix;
pub(crate) mod segment_append;
pub(crate) mod segment_files;
pub(crate) mod segment_read;
pub(crate) mod segment_rewrite;
pub(crate) mod segment_store;
pub(crate) mod shard_router;
pub(crate) mod wal;
