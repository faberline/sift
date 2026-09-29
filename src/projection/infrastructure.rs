//! The projections themselves: logging with its embedded text index, trace, and
//! metric with its on-disk chunk store, and the adapters that bind them and the
//! journal to service_projection.

pub(crate) mod logging_projection;
pub(crate) mod metric_chunk_store;
pub(crate) mod metric_projection;
pub(crate) mod metric_projection_apply;
pub(crate) mod metric_projection_maintenance;
pub(crate) mod metric_projection_query;
pub(crate) mod metric_projection_scan;
pub(crate) mod service_projection_adapter;
pub(crate) mod trace_projection;
