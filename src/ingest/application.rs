//! Admitting and appending: the per-project admission controller, the
//! retention check, governing decoded events, splitting them into Raft batches
//! and appending them through the group-commit queue, the local storage
//! preflight, and draining ingest on shutdown.

pub(crate) mod admission_controller;
pub(crate) mod append_events;
pub(crate) mod drain_ingest;
pub(crate) mod ensure_local_capacity;
pub(crate) mod raft_batch_splitter;
pub(crate) mod retention_admission;

pub(crate) mod admission_failure;
pub(crate) mod event_identity;
pub(crate) mod request_body_decoder;
