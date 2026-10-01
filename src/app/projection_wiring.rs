//! Assemble the shared projection runtime in the original registration order.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use crate::journal::application::journal_service::DurableJournal;
use crate::projection::application::projection_runtime::{
    ProjectionRuntime, PROJECTION_BATCH_SIZE, PROJECTION_RETRY_AFTER_SECONDS,
    PROJECTION_SNAPSHOT_INTERVAL_EVENTS,
};
use crate::projection::infrastructure::logging_projection::LoggingProjection;
use crate::projection::infrastructure::metric_projection::MetricProjection;
use crate::projection::infrastructure::replay_factory::NativeReplayFactory;
use crate::projection::infrastructure::service_projection_adapter::JournalProjectionSource;
use crate::projection::infrastructure::shared_runtime_backend::SharedRuntimeBackend;
use crate::projection::infrastructure::trace_projection::TraceProjection;
use crate::shared_kernel::stored_event::StoredEvent;

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
        Ok(Self::new(
            Arc::new(SharedRuntimeBackend {
                registry,
                logging,
                metrics,
                traces,
                retry_after_seconds: PROJECTION_RETRY_AFTER_SECONDS,
            }),
            Arc::new(NativeReplayFactory),
        ))
    }
}
