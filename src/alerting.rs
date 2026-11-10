//! Durable alert states and an outbox with stable notification IDs.
pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;
pub use application::service::{AlertRule, AlertState, AlertingService, Notification};
