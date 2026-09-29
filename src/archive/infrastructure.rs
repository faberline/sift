//! Where the archive lives: its control records on disk, the paged catalogs and
//! spill files, the GCS URIs and verified fetches, the segment cache, the
//! Parquet codec, integrity checks and private file modes.

pub(crate) mod archive_catalog;
pub(crate) mod archive_commit_state;
pub(crate) mod archive_control_paths;
pub(crate) mod archive_gc_pending_store;
pub(crate) mod archive_integrity;
pub(crate) mod archive_segment_cache;
pub(crate) mod archive_signal_stream;
pub(crate) mod archive_upload_intent;
pub(crate) mod dedupe_receipt_archive;
pub(crate) mod ephemeral_file_object_store;
pub(crate) mod gc_plan_builder;
pub(crate) mod gcs_uri;
pub(crate) mod local_blob_gc_store;
pub(crate) mod local_blob_live_index;
pub(crate) mod local_segment_commit_state;
pub(crate) mod parquet_event_codec;
pub(crate) mod private_fs_mode;
pub(crate) mod restore_state;
pub(crate) mod spill_catalog;
pub(crate) mod verified_manifest_fetcher;
