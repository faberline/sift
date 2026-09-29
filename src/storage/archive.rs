//! The archive's public paths. The archive context now owns the manifest, the
//! upload, eviction, expiration, GC, restore and replay use cases and their
//! control records; these re-exports keep `sift::storage::archive::*`
//! compiling for callers outside the crate.

pub use crate::archive::application::archive_journal_local::{
    archive_journal_local, archive_journal_local_captured,
};
pub use crate::archive::application::archive_journal_to_gcs::{
    archive_journal_gcs, archive_journal_gcs_captured,
};
pub use crate::archive::application::archive_raw_storage_to_gcs::archive_gcs;
pub use crate::archive::application::archive_receipts::{
    ArchiveCommitStatus, ArchiveReceipt, ArchiveReplay, ExpirationReceipt, HotEvictionReceipt,
    LocalArchiveReceipt,
};
pub use crate::archive::application::archive_status_queries::{
    committed_status, verify_committed_manifest_available,
};
pub use crate::archive::application::committed_event_reader::CommittedEventReader;
pub use crate::archive::application::evict_cold_segments::evict_committed_cold_segments_at;
pub use crate::archive::application::expire_committed_events::expire_committed_events_at;
pub use crate::archive::application::finalize_archive_gc::{
    finalize_archive_gc_after_checkpoint, finalize_archive_gc_batch_after_checkpoint,
};
pub use crate::archive::application::inspect_archive_catalog::{
    inspect_archive_catalog, inspect_archive_gc_plan,
};
pub use crate::archive::application::replay_committed_events::replay_committed_events;
pub use crate::archive::application::restore_gcs::{bootstrap_gcs_if_needed, restore_gcs};
pub use crate::archive::application::resume_local_blob_gc::resume_local_blob_gc_batch;
pub use crate::archive::application::retention_due::retention_due_at;
pub use crate::archive::domain::archive_manifest::{
    ArchiveBlob, ArchiveDedupeReceipt, ArchiveManifest, ArchiveRetentionDelta,
    ArchiveRetentionScan, ArchiveSegment,
};
pub use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
