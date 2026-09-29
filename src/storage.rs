//! Canonical raw storage plane: content-addressed blobs plus epoch-routed,
//! CRC-framed per-signal segments.

pub mod archive;
pub(crate) use crate::journal::domain::dedupe_receipt::DedupeReceipt;
pub use crate::journal::domain::journal_head::JournalHead;
pub use crate::journal::domain::retained_prefix_reconcile_stats::RetainedPrefixReconcileStats;
pub use crate::journal::domain::segment_manifest::{AppendLocation, SegmentManifest, SegmentState};
pub use crate::journal::domain::shard_route::{EpochMap, Route, VIRTUAL_BUCKETS};
pub use crate::journal::domain::storage_config::StorageConfig;
pub use crate::journal::infrastructure::storage::blob_store::BlobStore;
pub use crate::journal::infrastructure::storage::raw_storage::RawStorage;
pub use crate::journal::infrastructure::storage::wal::{SignalWal, SignalWalReader};
pub use crate::journal::{
    domain::idempotency_window::IDEMPOTENCY_WINDOW_SECONDS,
    infrastructure::storage::dedupe_index::DedupeIndex,
};
pub use crate::node::infrastructure::local_capacity::{
    CapacityLevel, LocalCapacity, LocalCapacityError,
};
pub use crate::node::{
    domain::storage_role::StorageRole,
    infrastructure::data_layout::{DataLayout, LayoutManifest, DEFAULT_DATA_DIR},
};
