//! Sift adapters for the shared typed projection runtime.

use std::sync::Arc;

use anyhow::Result;

use crate::projection::application::projection_runtime::Projection;
use crate::projection::infrastructure::logging_projection::LoggingProjection;
use crate::projection::infrastructure::metric_projection::MetricProjection;
use crate::projection::infrastructure::trace_projection::TraceProjection;
use crate::shared_kernel::stored_event::StoredEvent;
use crate::DurableJournal;

macro_rules! impl_shared_projection {
    ($projection:ty) => {
        impl service_projection::Projection<StoredEvent> for $projection {
            fn descriptor(&self) -> service_projection::ProjectionDescriptor {
                Projection::descriptor(self)
            }

            fn apply_idempotent(&self, event: &StoredEvent) -> Result<()> {
                Projection::apply_idempotent(self, event)
            }

            fn snapshot(&self) -> Result<Vec<u8>> {
                Projection::snapshot(self)
            }

            fn restore(&self, state: &[u8]) -> Result<()> {
                Projection::restore(self, state)
            }

            fn checkpoint_committed(&self) -> Result<()> {
                Projection::checkpoint_committed(self)
            }

            fn semantic_digest(&self) -> Result<String> {
                Projection::semantic_digest(self)
            }
        }
    };
}

impl_shared_projection!(LoggingProjection);

impl_shared_projection!(MetricProjection);

impl_shared_projection!(TraceProjection);

impl service_projection::ProjectionRecord for StoredEvent {
    fn projection_cursor(&self) -> u64 {
        self.cursor
    }

    fn projection_event_id(&self) -> &str {
        &self.event.event_id
    }
}

pub(in crate::projection) struct JournalProjectionSource {
    pub(in crate::projection) journal: Arc<DurableJournal>,
}

struct JournalProjectionSession {
    inner: crate::JournalProjectionReadSession,
}

impl service_projection::ProjectionReadSession<StoredEvent> for JournalProjectionSession {
    fn read_next(&mut self, limit: usize) -> Result<Vec<StoredEvent>> {
        self.inner.read_next(limit)
    }
}

impl service_projection::ProjectionSource<StoredEvent> for JournalProjectionSource {
    fn current_cursor(&self) -> u64 {
        self.journal.last_cursor()
    }

    fn read_after(&self, after: u64, limit: usize) -> Result<Vec<StoredEvent>> {
        self.journal.query_projection_events(after, limit)
    }

    fn open_read_session(
        &self,
        after: u64,
    ) -> Result<Option<Box<dyn service_projection::ProjectionReadSession<StoredEvent>>>> {
        Ok(Some(Box::new(JournalProjectionSession {
            inner: self.journal.projection_read_session(after)?,
        })))
    }

    fn generation(&self) -> u64 {
        self.journal.projection_generation()
    }
}
