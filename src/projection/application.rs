//! The projection runtime Sift reads through: the Projection hook each
//! projection implements, the runtime that keeps the logging, metric and trace
//! projections caught up with the journal, and the log record normalizer.

pub(crate) mod log_record_normalizer;
pub(crate) mod projection_runtime;
