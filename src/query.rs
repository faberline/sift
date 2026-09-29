//! Queries: the versioned query API over logs, metrics and traces, its async
//! jobs, the log tail, service listing and correlation, trace lookup, the
//! Prometheus-compatible query endpoints, the query role that forwards to the
//! store, and the read-only MCP tools over the same API.

pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;
