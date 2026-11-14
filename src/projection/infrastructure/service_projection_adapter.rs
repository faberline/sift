//! Sift adapters for the shared typed projection runtime.

use std::sync::Arc;

use service_projection::{
    ProjectionCursor, ProjectionError, ProjectionEventId, ProjectionName, SourceGeneration,
};

use crate::journal::application::journal_service::DurableJournal;
use crate::projection::domain::projection::Projection;
use crate::projection::infrastructure::logging_projection::LoggingProjection;
use crate::projection::infrastructure::metric_projection::MetricProjection;
use crate::projection::infrastructure::trace_projection::TraceProjection;
use crate::shared_kernel::stored_event::StoredEvent;

macro_rules! impl_shared_projection {
    ($projection:ty) => {
        impl service_projection::Projection<StoredEvent> for $projection {
            fn descriptor(&self) -> service_projection::ProjectionDescriptor {
                let descriptor = Projection::descriptor(self);
                service_projection::ProjectionDescriptor::try_new(
                    ProjectionName::new(descriptor.name),
                    descriptor.schema_version,
                    descriptor.retention,
                )
                .expect("Sift projection descriptors have valid names")
            }

            fn apply_idempotent(&self, event: &StoredEvent) -> Result<(), ProjectionError> {
                Projection::apply_idempotent(self, event).map_err(ProjectionError::other)
            }

            fn snapshot(&self) -> Result<Vec<u8>, ProjectionError> {
                Projection::snapshot(self).map_err(ProjectionError::other)
            }

            fn restore(&self, state: &[u8]) -> Result<(), ProjectionError> {
                Projection::restore(self, state).map_err(ProjectionError::other)
            }

            fn checkpoint_committed(&self) -> Result<(), ProjectionError> {
                Projection::checkpoint_committed(self).map_err(ProjectionError::other)
            }

            fn semantic_digest(&self) -> Result<String, ProjectionError> {
                Projection::semantic_digest(self).map_err(ProjectionError::other)
            }
        }
    };
}

impl_shared_projection!(LoggingProjection);

impl_shared_projection!(MetricProjection);

impl_shared_projection!(TraceProjection);

impl service_projection::ProjectionRecord for StoredEvent {
    fn projection_cursor(&self) -> ProjectionCursor {
        ProjectionCursor::new(self.cursor)
    }

    fn projection_event_id(&self) -> ProjectionEventId {
        ProjectionEventId::new(self.event.event_id.clone())
    }
}

pub(crate) struct JournalProjectionSource {
    pub(crate) journal: Arc<DurableJournal>,
}

struct JournalProjectionSession {
    inner:
        crate::journal::application::journal_projection_read_session::JournalProjectionReadSession,
}

impl service_projection::ProjectionReadSession<StoredEvent> for JournalProjectionSession {
    fn read_next(&mut self, limit: usize) -> Result<Vec<StoredEvent>, ProjectionError> {
        self.inner.read_next(limit).map_err(ProjectionError::other)
    }
}

impl service_projection::ProjectionSource<StoredEvent> for JournalProjectionSource {
    fn current_cursor(&self) -> ProjectionCursor {
        ProjectionCursor::new(self.journal.last_cursor())
    }

    fn read_after(
        &self,
        after: ProjectionCursor,
        limit: usize,
    ) -> Result<Vec<StoredEvent>, ProjectionError> {
        self.journal
            .query_projection_events(after.get(), limit)
            .map_err(ProjectionError::other)
    }

    fn open_read_session(
        &self,
        after: ProjectionCursor,
    ) -> Result<
        Option<Box<dyn service_projection::ProjectionReadSession<StoredEvent>>>,
        ProjectionError,
    > {
        let inner = self
            .journal
            .projection_read_session(after.get())
            .map_err(ProjectionError::other)?;
        Ok(Some(Box::new(JournalProjectionSession { inner })))
    }

    fn generation(&self) -> SourceGeneration {
        SourceGeneration::new(self.journal.projection_generation())
    }
}
