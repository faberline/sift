//! Adapter for the original shared runtime and its typed projection handles.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::projection::domain::log::{LogPage, LogQuery, PROJECTION_LOGGING_STORE};
use crate::projection::domain::metric::{MetricPage, MetricQuery, PROJECTION_METRIC_STORE};
use crate::projection::domain::runtime_metadata::{ProjectionLag, RebuildComparison};
use crate::projection::domain::runtime_port::ProjectionRuntimePort;
use crate::projection::domain::trace::{
    TracePage, TraceQuery, TraceResultV1, PROJECTION_TRACE_STORE,
};
use crate::projection::infrastructure::logging_projection::LoggingProjection;
use crate::projection::infrastructure::metric_projection::MetricProjection;
use crate::projection::infrastructure::trace_projection::TraceProjection;
use crate::shared_kernel::port_future::PortFuture;
use crate::shared_kernel::stored_event::StoredEvent;

pub(crate) struct SharedRuntimeBackend {
    pub(crate) registry: service_projection::ProjectionRegistry<StoredEvent>,
    pub(crate) logging: Arc<service_projection::ProjectionHandle<StoredEvent, LoggingProjection>>,
    pub(crate) metrics: Arc<service_projection::ProjectionHandle<StoredEvent, MetricProjection>>,
    pub(crate) traces: Arc<service_projection::ProjectionHandle<StoredEvent, TraceProjection>>,
    pub(crate) retry_after_seconds: u64,
}

impl ProjectionRuntimePort for SharedRuntimeBackend {
    type Error = anyhow::Error;
    fn projection_names(&self) -> Vec<String> {
        self.registry.projection_names()
    }

    fn has_projection(&self, name: &str) -> bool {
        self.registry.has_projection(name)
    }

    fn current_cursor(&self, name: &str) -> Result<u64> {
        self.registry.current_cursor(name)
    }

    fn semantic_digest(&self, name: &str) -> Result<String> {
        self.registry.semantic_digest(name)
    }

    fn query_logs(&self, query: &LogQuery) -> Result<LogPage> {
        self.logging.catch_up()?;
        self.logging.projection().query(query)
    }

    fn get_trace(&self, project: &str, trace_id: &str) -> Result<Option<TraceResultV1>> {
        self.traces.catch_up()?;
        self.traces.projection().get_trace(project, trace_id)
    }

    fn query_traces(&self, query: &TraceQuery) -> Result<TracePage> {
        self.traces.catch_up()?;
        self.traces.projection().query(query)
    }

    fn query_metrics(&self, query: &MetricQuery) -> Result<MetricPage> {
        self.metrics.catch_up()?;
        self.metrics.projection().query(query)
    }

    fn catch_up(&self, name: &str) -> Result<u64> {
        self.registry.catch_up(name)
    }

    fn wait_for_min_cursor<'a>(
        &'a self,
        name: &'a str,
        required_cursor: u64,
        timeout: Duration,
    ) -> PortFuture<'a, u64, ProjectionLag> {
        Box::pin(async move {
            let result = match name {
                PROJECTION_LOGGING_STORE => {
                    self.logging
                        .wait_for_min_cursor(required_cursor, timeout)
                        .await
                }
                PROJECTION_METRIC_STORE => {
                    self.metrics
                        .wait_for_min_cursor(required_cursor, timeout)
                        .await
                }
                PROJECTION_TRACE_STORE => {
                    self.traces
                        .wait_for_min_cursor(required_cursor, timeout)
                        .await
                }
                _ => {
                    return Err(ProjectionLag::new(
                        name,
                        required_cursor,
                        0,
                        self.retry_after_seconds,
                    ))
                }
            };
            result.map_err(|lag| ProjectionLag {
                error: lag.error,
                projection: lag.projection,
                required_cursor: lag.required_cursor,
                current_cursor: lag.current_cursor,
                retryable: lag.retryable,
                retry_after_seconds: lag.retry_after_seconds,
            })
        })
    }

    fn rebuild_and_compare(&self, name: &str) -> Result<RebuildComparison> {
        let result = self.registry.rebuild_and_compare(name)?;
        Ok(RebuildComparison {
            source_cursor: result.source_cursor,
            rebuilt_cursor: result.rebuilt_cursor,
            live_digest: result.live_digest,
            rebuilt_digest: result.rebuilt_digest,
            equal: result.equal,
        })
    }

    fn persist_all(&self) -> Result<()> {
        self.registry
            .flush_all()
            .context("flush typed Sift projections")
    }
}
