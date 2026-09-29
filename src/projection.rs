//! Projections: the logging, metric and trace read models Sift builds from the
//! journal, kept caught up by service_projection's runtime, snapshotted and
//! rebuilt, and queried by the API.

pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;

pub use application::projection_runtime::{
    Projection, ProjectionRuntime, PROJECTION_BATCH_SIZE, PROJECTION_RETRY_AFTER_SECONDS,
    PROJECTION_SNAPSHOT_INTERVAL_EVENTS,
};
pub use domain::log::{
    LogPage, LogQuery, LogRecordV1, DEFAULT_RETAINED_LOG_RECORDS, LOGGING_SCHEMA_VERSION,
    MAX_LOG_QUERY_LIMIT, PROJECTION_LOGGING_STORE,
};
pub use domain::metric::{
    HistogramKind, MetricAggregation, MetricChunkV1, MetricHistogramV1, MetricPage, MetricPointV1,
    MetricQuery, MetricRollupV1, MetricSeriesResultV1, DEFAULT_METRIC_CARDINALITY_LIMIT,
    DEFAULT_RETAINED_POINTS_PER_SERIES, MAX_METRIC_QUERY_LIMIT, METRIC_CHUNK_POINTS,
    METRIC_SCHEMA_VERSION, PROJECTION_METRIC_STORE, ROLLUP_WINDOWS_SECONDS,
};
pub use domain::trace::{
    SpanEventV1, SpanLinkV1, SpanRecordV1, TracePage, TraceQuery, TraceResultV1,
    DEFAULT_RETAINED_TRACE_SPANS, MAX_TRACE_QUERY_LIMIT, PROJECTION_TRACE_STORE,
    TRACE_SCHEMA_VERSION,
};
pub use infrastructure::logging_projection::LoggingProjection;
pub use infrastructure::metric_projection::MetricProjection;
pub use infrastructure::trace_projection::TraceProjection;
pub use service_projection::{
    ProjectionCheckpoint, ProjectionDescriptor, ProjectionLag, ProjectionStateEnvelope,
    RebuildComparison, PROJECTION_STATE_FORMAT_VERSION,
};
