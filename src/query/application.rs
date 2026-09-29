//! Running a query: executing a versioned query against the projections and the
//! archive, reading a job back for a project, and building and evaluating a
//! Prometheus metric query.

pub(crate) mod archive_query_status;
pub(crate) mod execute_query;
pub(crate) mod get_query_job;
pub(crate) mod prom_query;
