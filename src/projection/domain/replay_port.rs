//! Temporary projection replay contracts used for cold reads.

use std::sync::Arc;

use crate::projection::domain::log::{LogPage, LogQuery};
use crate::projection::domain::metric::{MetricPage, MetricQuery};
use crate::projection::domain::trace::{TracePage, TraceQuery};
use crate::shared_kernel::stored_event::StoredEvent;

pub(crate) trait ReplayProjection<Q, P>: Send + Sync {
    type Error;
    fn apply_idempotent(&self, event: &StoredEvent) -> Result<(), Self::Error>;
    fn query(&self, query: &Q) -> Result<P, Self::Error>;
}

pub(crate) type ReplayHandle<Q, P, E> = Arc<dyn ReplayProjection<Q, P, Error = E>>;

pub(crate) trait ReplayFactory: Send + Sync {
    type Error;
    fn logs(&self) -> Result<ReplayHandle<LogQuery, LogPage, Self::Error>, Self::Error>;
    fn metrics(&self) -> ReplayHandle<MetricQuery, MetricPage, Self::Error>;
    fn traces(&self) -> ReplayHandle<TraceQuery, TracePage, Self::Error>;
}
