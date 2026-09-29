//! What the three projections keep and answer: the log record, span and metric
//! point schemas, their queries and pages, the state each projection holds, and
//! the rules over it: log filtering, trace topology and correlations, and
//! metric series identity, chunks, memory size, rollups and aggregation.

pub(crate) mod log;
pub(crate) mod metric;
pub(crate) mod metric_aggregation;
pub(crate) mod metric_memory_size;
pub(crate) mod metric_series;
pub(crate) mod trace;
