//! Cold read adapters use the existing concrete projection implementations.

use std::sync::Arc;

use anyhow::Result;

use crate::projection::domain::log::{LogPage, LogQuery};
use crate::projection::domain::metric::{MetricPage, MetricQuery};
use crate::projection::domain::projection::Projection;
use crate::projection::domain::replay_port::{ReplayFactory, ReplayHandle, ReplayProjection};
use crate::projection::domain::trace::{TracePage, TraceQuery};
use crate::projection::infrastructure::logging_projection::LoggingProjection;
use crate::projection::infrastructure::metric_projection::MetricProjection;
use crate::projection::infrastructure::trace_projection::TraceProjection;
use crate::shared_kernel::stored_event::StoredEvent;

pub(crate) struct NativeReplayFactory;

impl ReplayFactory for NativeReplayFactory {
    type Error = anyhow::Error;
    fn logs(&self) -> Result<ReplayHandle<LogQuery, LogPage, Self::Error>> {
        Ok(Arc::new(LoggingProjection::new()?))
    }
    fn metrics(&self) -> ReplayHandle<MetricQuery, MetricPage, Self::Error> {
        Arc::new(MetricProjection::new())
    }
    fn traces(&self) -> ReplayHandle<TraceQuery, TracePage, Self::Error> {
        Arc::new(TraceProjection::new())
    }
}

macro_rules! replay_adapter {
    ($projection:ty, $query:ty, $page:ty) => {
        impl ReplayProjection<$query, $page> for $projection {
            type Error = anyhow::Error;
            fn apply_idempotent(&self, event: &StoredEvent) -> Result<()> {
                Projection::apply_idempotent(self, event)
            }
            fn query(&self, query: &$query) -> Result<$page> {
                <$projection>::query(self, query)
            }
        }
    };
}

replay_adapter!(LoggingProjection, LogQuery, LogPage);
replay_adapter!(MetricProjection, MetricQuery, MetricPage);
replay_adapter!(TraceProjection, TraceQuery, TracePage);
