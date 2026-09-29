//! The published language every context shares: the versioned operational
//! event, the stored event a cursor points at, the event-content digest, the
//! archive watermarks, the 180-day retention boundary and the Cloud Logging
//! event id.

pub(crate) mod archive_watermarks;
pub(crate) mod cloud_logging_event_id;
pub(crate) mod event;
pub(crate) mod event_content_digest;
pub(crate) mod retention_boundary;
pub(crate) mod single_signal_batch;
pub(crate) mod stored_event;
