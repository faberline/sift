//! Canonical raw storage plane: content-addressed blobs plus epoch-routed,
//! CRC-framed per-signal segments.

pub mod archive;
mod capacity;
mod layout;

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
pub use capacity::{CapacityLevel, LocalCapacity, LocalCapacityError};
pub use layout::{DataLayout, LayoutManifest, StorageRole, DEFAULT_DATA_DIR};

pub(crate) trait BlobHashSet {
    fn insert_hash(&mut self, hash: &str) -> anyhow::Result<()>;
    fn contains_hash(&self, hash: &str) -> anyhow::Result<bool>;
}
