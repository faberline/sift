//! The journal: Sift's durable, replicated event log. It accepts governed event
//! batches through Raft, deduplicates them inside the idempotency window, keeps
//! a resident window of recent events, recovers from the WAL and sealed
//! segments, adopts archive checkpoints and retention, and serves reads to the
//! projections and queries.

pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;

pub use crate::journal::{
    domain::{
        journal_head::JournalHead,
        raft_batch_limits::{RAFT_BATCH_MAX_BYTES, RAFT_BATCH_MAX_DELAY, RAFT_BATCH_MAX_ITEMS},
        storage_config::StorageConfig,
    },
    infrastructure::{
        raft::{
            diagnostics::{
                decode_raft_batch_for_diagnostics,
                encode_archive_checkpoint_barrier_for_diagnostics,
                encode_clear_retention_fence_for_diagnostics,
                encode_default_raft_batch_for_diagnostics, encode_raft_batch_at_for_diagnostics,
                encode_raft_batch_for_diagnostics,
            },
            sift_state_machine::SiftStateMachine,
            snapshot_format::SNAPSHOT_CONTENT_TYPE,
        },
        storage::{
            blob_store::BlobStore,
            dedupe_index::DedupeIndex,
            raw_storage::RawStorage,
            wal::{SignalWal, SignalWalReader},
        },
    },
};

pub use crate::shared_kernel::{
    idempotency_window::IDEMPOTENCY_WINDOW_SECONDS,
    retained_prefix_reconcile_stats::RetainedPrefixReconcileStats,
    segment_manifest::{AppendLocation, SegmentManifest, SegmentState},
    shard_route::{EpochMap, Route, VIRTUAL_BUCKETS},
};
