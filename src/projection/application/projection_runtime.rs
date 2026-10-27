//! Application projection reads and control over the domain ports.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;

use crate::projection::application::replay_service::ReplayService;
use crate::projection::domain::log::{LogPage, LogQuery};
use crate::projection::domain::metric::{MetricPage, MetricQuery};
use crate::projection::domain::replay_port::ReplayFactory;
pub use crate::projection::domain::runtime_metadata::{ProjectionLag, RebuildComparison};
use crate::projection::domain::runtime_port::ProjectionRuntimePort;
use crate::projection::domain::trace::{TracePage, TraceQuery, TraceResultV1};

pub const PROJECTION_BATCH_SIZE: usize = 1_000;
pub const PROJECTION_RETRY_AFTER_SECONDS: u64 = 1;
pub const PROJECTION_SNAPSHOT_INTERVAL_EVENTS: u64 = 100_000;

pub struct ProjectionRuntime {
    port: Arc<dyn ProjectionRuntimePort<Error = anyhow::Error>>,
    replay: Arc<dyn ReplayFactory<Error = anyhow::Error>>,
}

impl ProjectionRuntime {
    pub(crate) fn new(
        port: Arc<dyn ProjectionRuntimePort<Error = anyhow::Error>>,
        replay: Arc<dyn ReplayFactory<Error = anyhow::Error>>,
    ) -> Self {
        Self { port, replay }
    }

    pub fn projection_names(&self) -> Vec<String> {
        self.port.projection_names()
    }
    pub fn has_projection(&self, name: &str) -> bool {
        self.port.has_projection(name)
    }
    pub fn current_cursor(&self, name: &str) -> Result<u64> {
        self.port.current_cursor(name)
    }
    pub fn semantic_digest(&self, name: &str) -> Result<String> {
        self.port.semantic_digest(name)
    }
    pub fn query_logs(&self, query: &LogQuery) -> Result<LogPage> {
        self.port.query_logs(query)
    }
    pub fn get_trace(&self, project: &str, trace_id: &str) -> Result<Option<TraceResultV1>> {
        self.port.get_trace(project, trace_id)
    }
    pub fn query_traces(&self, query: &TraceQuery) -> Result<TracePage> {
        self.port.query_traces(query)
    }
    pub fn query_metrics(&self, query: &MetricQuery) -> Result<MetricPage> {
        self.port.query_metrics(query)
    }
    pub fn catch_up(&self, name: &str) -> Result<u64> {
        self.port.catch_up(name)
    }
    pub async fn wait_for_min_cursor(
        &self,
        name: &str,
        required_cursor: u64,
        timeout: Duration,
    ) -> std::result::Result<u64, ProjectionLag> {
        self.port
            .wait_for_min_cursor(name, required_cursor, timeout)
            .await
    }
    pub fn rebuild_and_compare(&self, name: &str) -> Result<RebuildComparison> {
        self.port.rebuild_and_compare(name)
    }
    pub fn persist_all(&self) -> Result<()> {
        self.port.persist_all()
    }

    pub(crate) fn new_log_replay(&self) -> Result<ReplayService<LogQuery, LogPage>> {
        self.replay.logs().map(ReplayService::new)
    }
    pub(crate) fn new_metric_replay(&self) -> ReplayService<MetricQuery, MetricPage> {
        ReplayService::new(self.replay.metrics())
    }
    pub(crate) fn new_trace_replay(&self) -> ReplayService<TraceQuery, TracePage> {
        ReplayService::new(self.replay.traces())
    }
}
