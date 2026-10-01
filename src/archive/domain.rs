//! What an archive is made of: the manifest with its segments, blobs, dedupe
//! receipts and retention delta, its validation, the catalog rows, the event
//! set digests and times, and the retention and lifecycle settings.

pub(crate) mod archive_catalog_item;
pub(crate) mod archive_event_time;
pub(crate) mod archive_manifest_validator;
pub(crate) mod blob_hash_set;
pub(crate) mod event_set_digest;
pub(crate) mod lifecycle_settings;
pub(crate) mod manifest_validation_policy;
pub(crate) mod portable_manifest;
pub(crate) mod retention_policy;

pub(crate) mod lifecycle_capacity;

pub(crate) mod control_paths;
pub(crate) mod control_values;

pub(crate) mod commit_store_port;

pub(crate) mod upload_store_port;

pub(crate) mod gc_store_port;

pub(crate) mod manifest_store_port;

pub(crate) mod segment_codec_port;

pub(crate) mod restore_store_port;

pub(crate) mod file_modes_port;

pub(crate) mod resource_ports;

pub(crate) mod catalog_decode;

pub(crate) mod gcs_uri;
