//! What the archive does: committing remote and local archives, uploading
//! snapshots, evicting and expiring segments, reconciling retention, reporting
//! status, replaying committed events, collecting garbage, restoring volumes
//! and running the lifecycle steps.

pub(crate) mod archive_journal_local;
pub(crate) mod archive_journal_to_gcs;
pub(crate) mod archive_raw_storage_to_gcs;
pub(crate) mod archive_receipts;
pub(crate) mod archive_status_queries;
pub(crate) mod committed_event_reader;
pub(crate) mod compact_journal;
pub(crate) mod evict_cold_segments;
pub(crate) mod expire_committed_events;
pub(crate) mod finalize_archive_gc;
pub(crate) mod inspect_archive_catalog;
pub(crate) mod prepare_retention_fence;
pub(crate) mod reconcile_committed_retention;
pub(crate) mod replay_cold_query;
pub(crate) mod replay_committed_events;
pub(crate) mod replay_recent_committed;
pub(crate) mod restore_gcs;
pub(crate) mod restore_into_empty;
pub(crate) mod resume_local_blob_gc;
pub(crate) mod retention_catalog_ops;
pub(crate) mod retention_due;
pub(crate) mod retention_manifest_commit;
pub(crate) mod run_lifecycle_attempt;
pub(crate) mod upload_archive_snapshot;
