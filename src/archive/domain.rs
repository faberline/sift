//! What an archive is made of: the manifest with its segments, blobs, dedupe
//! receipts and retention delta, its validation, the catalog rows, the event
//! set digests and times, and the retention and lifecycle settings.

pub(crate) mod archive_catalog_item;
pub(crate) mod archive_event_time;
pub(crate) mod archive_manifest;
pub(crate) mod archive_manifest_validator;
pub(crate) mod blob_hash_set;
pub(crate) mod event_set_digest;
pub(crate) mod lifecycle_settings;
pub(crate) mod portable_manifest;
pub(crate) mod retention_policy;
