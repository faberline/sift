//! The Projection hook Sift's projections implement, and the runtime that
//! registers the logging, metric and trace projections, keeps them caught up
//! with the journal and answers their queries.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use service_projection::{ProjectionLag, RebuildComparison};
use sha2::{Digest, Sha256};

use crate::projection::domain::log::{LogPage, LogQuery, PROJECTION_LOGGING_STORE};
use crate::projection::domain::metric::{MetricPage, MetricQuery, PROJECTION_METRIC_STORE};
use crate::projection::domain::trace::{
    TracePage, TraceQuery, TraceResultV1, PROJECTION_TRACE_STORE,
};
use crate::projection::infrastructure::logging_projection::LoggingProjection;
use crate::projection::infrastructure::metric_projection::MetricProjection;
use crate::projection::infrastructure::service_projection_adapter::JournalProjectionSource;
use crate::projection::infrastructure::trace_projection::TraceProjection;
use crate::shared_kernel::stored_event::StoredEvent;
use crate::DurableJournal;

pub const PROJECTION_BATCH_SIZE: usize = 1_000;
pub const PROJECTION_RETRY_AFTER_SECONDS: u64 = 1;
pub const PROJECTION_SNAPSHOT_INTERVAL_EVENTS: u64 = 100_000;

/// Sift's domain projection hook. The shared runtime owns its control flow.
pub trait Projection: Send + Sync + 'static {
    fn descriptor(&self) -> service_projection::ProjectionDescriptor;
    fn apply_idempotent(&self, event: &StoredEvent) -> Result<()>;
    fn snapshot(&self) -> Result<Vec<u8>>;
    fn restore(&self, state: &[u8]) -> Result<()>;

    fn checkpoint_committed(&self) -> Result<()> {
        Ok(())
    }

    fn semantic_digest(&self) -> Result<String> {
        Ok(hex::encode(Sha256::digest(self.snapshot()?)))
    }
}

pub struct ProjectionRuntime {
    registry: service_projection::ProjectionRegistry<StoredEvent>,
    logging: Arc<service_projection::ProjectionHandle<StoredEvent, LoggingProjection>>,
    metrics: Arc<service_projection::ProjectionHandle<StoredEvent, MetricProjection>>,
    traces: Arc<service_projection::ProjectionHandle<StoredEvent, TraceProjection>>,
}

impl ProjectionRuntime {
    pub fn open(data_dir: impl AsRef<Path>, journal: Arc<DurableJournal>) -> Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        let source: Arc<dyn service_projection::ProjectionSource<StoredEvent>> =
            Arc::new(JournalProjectionSource { journal });
        let config = service_projection::ProjectionRuntimeConfig::new(
            PROJECTION_BATCH_SIZE,
            PROJECTION_SNAPSHOT_INTERVAL_EVENTS,
            PROJECTION_RETRY_AFTER_SECONDS,
        );
        let mut registry = service_projection::ProjectionRegistry::new(&data_dir, source, config)?;
        let logging = registry.register(|| Ok(Arc::new(LoggingProjection::new()?)))?;
        let metric_data_dir = data_dir.clone();
        let metrics =
            registry.register(move || Ok(Arc::new(MetricProjection::open(&metric_data_dir)?)))?;
        let traces = registry.register(|| Ok(Arc::new(TraceProjection::new())))?;
        Ok(Self {
            registry,
            logging,
            metrics,
            traces,
        })
    }

    pub fn projection_names(&self) -> Vec<String> {
        self.registry.projection_names()
    }

    pub fn has_projection(&self, name: &str) -> bool {
        self.registry.has_projection(name)
    }

    pub fn current_cursor(&self, name: &str) -> Result<u64> {
        self.registry.current_cursor(name)
    }

    pub fn semantic_digest(&self, name: &str) -> Result<String> {
        self.registry.semantic_digest(name)
    }

    pub fn query_logs(&self, query: &LogQuery) -> Result<LogPage> {
        self.logging.catch_up()?;
        self.logging.projection().query(query)
    }

    pub fn get_trace(&self, project: &str, trace_id: &str) -> Result<Option<TraceResultV1>> {
        self.traces.catch_up()?;
        self.traces.projection().get_trace(project, trace_id)
    }

    pub fn query_traces(&self, query: &TraceQuery) -> Result<TracePage> {
        self.traces.catch_up()?;
        self.traces.projection().query(query)
    }

    pub fn query_metrics(&self, query: &MetricQuery) -> Result<MetricPage> {
        self.metrics.catch_up()?;
        self.metrics.projection().query(query)
    }

    pub fn catch_up(&self, name: &str) -> Result<u64> {
        self.registry.catch_up(name)
    }

    pub async fn wait_for_min_cursor(
        &self,
        name: &str,
        required_cursor: u64,
        timeout: Duration,
    ) -> std::result::Result<u64, ProjectionLag> {
        match name {
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
            _ => Err(ProjectionLag::new(
                name,
                required_cursor,
                0,
                PROJECTION_RETRY_AFTER_SECONDS,
            )),
        }
    }

    pub fn rebuild_and_compare(&self, name: &str) -> Result<RebuildComparison> {
        self.registry.rebuild_and_compare(name)
    }

    pub fn persist_all(&self) -> Result<()> {
        self.registry
            .flush_all()
            .context("flush typed Sift projections")
    }
}
