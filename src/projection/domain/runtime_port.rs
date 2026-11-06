//! Projection reads and runtime control without a storage implementation.

use std::time::Duration;

use crate::projection::domain::log::{LogPage, LogQuery};
use crate::projection::domain::metric::{MetricPage, MetricQuery};
use crate::projection::domain::runtime_metadata::{ProjectionLag, RebuildComparison};
use crate::projection::domain::trace::{TracePage, TraceQuery, TraceResultV1};
use crate::shared_kernel::port_future::PortFuture;

pub(crate) trait ProjectionRuntimePort: Send + Sync {
    type Error;

    fn projection_names(&self) -> Vec<String>;
    fn has_projection(&self, name: &str) -> bool;
    fn current_cursor(&self, name: &str) -> Result<u64, Self::Error>;
    fn semantic_digest(&self, name: &str) -> Result<String, Self::Error>;
    fn query_logs(&self, query: &LogQuery) -> Result<LogPage, Self::Error>;
    fn get_trace(
        &self,
        project: &str,
        trace_id: &str,
    ) -> Result<Option<TraceResultV1>, Self::Error>;
    fn query_traces(&self, query: &TraceQuery) -> Result<TracePage, Self::Error>;
    fn query_metrics(&self, query: &MetricQuery) -> Result<MetricPage, Self::Error>;
    fn catch_up(&self, name: &str) -> Result<u64, Self::Error>;
    fn wait_for_min_cursor<'a>(
        &'a self,
        name: &'a str,
        required_cursor: u64,
        timeout: Duration,
    ) -> PortFuture<'a, u64, ProjectionLag>;
    fn rebuild_and_compare(&self, name: &str) -> Result<RebuildComparison, Self::Error>;
    fn persist_all(&self) -> Result<(), Self::Error>;
}
