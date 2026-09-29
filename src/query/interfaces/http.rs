//! The query HTTP endpoints and their wire schemas: the versioned query and its
//! jobs, the log tail, service listing, correlation, trace lookup, the
//! Prometheus-compatible endpoints, and the query role's router.

pub(crate) mod correlate_v1;
pub(crate) mod get_trace;
pub(crate) mod list_services_v1;
pub(crate) mod phase_one;
pub(crate) mod prom_query_params;
pub(crate) mod prometheus_query;
pub(crate) mod query_request_v1;
pub(crate) mod query_response_v1;
pub(crate) mod query_role_router;
pub(crate) mod query_v1;
