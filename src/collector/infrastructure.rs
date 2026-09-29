//! The sources the collector reads (a file, stdin, a CRI log root) and their
//! checkpoints, the mapping from a service-log line to an operational event,
//! and the OTLP client that delivers the events to Sift.

pub(crate) mod checkpoint;
pub(crate) mod client;
pub(crate) mod cri_checkpoint;
pub(crate) mod cri_discovery;
pub(crate) mod cri_source;
pub(crate) mod service_log_mapper;
pub(crate) mod source;
