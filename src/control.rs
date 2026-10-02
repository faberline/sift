//! Durable product configuration and operation state in the control Raft group.
pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;
pub use application::service::{ControlRecord, ControlService, RecordKind};
