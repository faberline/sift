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

pub use crate::app::archive_wiring::{
    archive_gcs, archive_journal_gcs, archive_journal_gcs_captured, archive_journal_local,
    archive_journal_local_captured, bootstrap_gcs_if_needed, committed_status,
    evict_committed_cold_segments_at, expire_committed_events_at,
    finalize_archive_gc_after_checkpoint, finalize_archive_gc_batch_after_checkpoint,
    replay_committed_events, restore_gcs, resume_local_blob_gc_batch, retention_due_at,
    verify_committed_manifest_available,
};
pub use crate::archive::application::committed_event_reader::CommittedEventReader;
pub use crate::archive::application::inspect_archive_catalog::{
    inspect_archive_catalog, inspect_archive_gc_plan,
};
pub use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
pub use crate::shared_kernel::catalog_manifest::{CatalogPageRef, CatalogRoot};

pub use crate::shared_kernel::{
    archive_manifest::{
        ArchiveBlob, ArchiveDedupeReceipt, ArchiveManifest, ArchiveRetentionDelta,
        ArchiveRetentionScan, ArchiveSegment,
    },
    archive_receipts::{
        ArchiveCommitStatus, ArchiveReceipt, ArchiveReplay, ExpirationReceipt, HotEvictionReceipt,
        LocalArchiveReceipt,
    },
};
