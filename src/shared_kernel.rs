//! The published language every context shares: the versioned operational
//! event, the stored event a cursor points at, the event-content digest, the
//! archive watermarks, the 180-day retention boundary, the Cloud Logging
//! event id, the default query limit, the SHA-256 hex digest and UTF-8
//! truncation.

pub(crate) mod archive_watermarks;
pub(crate) mod catalog_manifest;
pub(crate) mod cloud_logging_event_id;
pub(crate) mod event;
pub(crate) mod event_content_digest;
pub(crate) mod event_error;
pub(crate) mod query_limit;
pub(crate) mod retention_boundary;
pub(crate) mod sha256_hex;
pub(crate) mod single_signal_batch;
pub(crate) mod stored_event;
pub(crate) mod utf8_truncate;
