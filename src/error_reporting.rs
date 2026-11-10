//! Durable error groups built from the retained log feed.
pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod interfaces;
pub use application::service::{ErrorGroup, ErrorReportingService, ErrorStatus};
