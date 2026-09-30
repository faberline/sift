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

pub use crate::archive::{
    application::{
        archive_journal_local::{archive_journal_local, archive_journal_local_captured},
        archive_journal_to_gcs::{archive_journal_gcs, archive_journal_gcs_captured},
        archive_raw_storage_to_gcs::archive_gcs,
        archive_receipts::{
            ArchiveCommitStatus, ArchiveReceipt, ArchiveReplay, ExpirationReceipt,
            HotEvictionReceipt, LocalArchiveReceipt,
        },
        archive_status_queries::{committed_status, verify_committed_manifest_available},
        committed_event_reader::CommittedEventReader,
        evict_cold_segments::evict_committed_cold_segments_at,
        finalize_archive_gc::{
            finalize_archive_gc_after_checkpoint, finalize_archive_gc_batch_after_checkpoint,
        },
        inspect_archive_catalog::{inspect_archive_catalog, inspect_archive_gc_plan},
        replay_committed_events::replay_committed_events,
        restore_gcs::{bootstrap_gcs_if_needed, restore_gcs},
        resume_local_blob_gc::resume_local_blob_gc_batch,
        retention_due::retention_due_at,
        retention_expiration::expire_committed_events_at,
    },
    domain::archive_manifest::{
        ArchiveBlob, ArchiveDedupeReceipt, ArchiveManifest, ArchiveRetentionDelta,
        ArchiveRetentionScan, ArchiveSegment,
    },
};
pub use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
