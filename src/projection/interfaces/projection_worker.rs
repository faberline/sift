//! The handle to the projection worker ServiceState starts. Stopping it ends
//! the catch-up loop, then flushes the journal's dedupe window and persists
//! every projection.

use std::sync::Arc;

use chrono::Utc;

use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::projection::application::projection_runtime::ProjectionRuntime;

pub struct ProjectionWorker {
    pub(crate) shutdown: Option<tokio::sync::watch::Sender<bool>>,
    pub(crate) task: tokio::task::JoinHandle<()>,
    pub(crate) projections: Arc<ProjectionRuntime>,
    pub(crate) journal: Arc<DurableJournal>,
}

impl ProjectionWorker {
    pub async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(true);
        }
        let _ = self.task.await;
        let journal = self.journal;
        match tokio::task::spawn_blocking(move || journal.maintain_dedupe_at(Utc::now(), true))
            .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::warn!(%error, "dedupe shutdown flush failed"),
            Err(error) => tracing::warn!(%error, "dedupe shutdown flush task panicked"),
        }
        let projections = self.projections;
        match tokio::task::spawn_blocking(move || projections.persist_all()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error, "projection shutdown flush failed"),
            Err(error) => tracing::warn!(%error, "projection shutdown flush task panicked"),
        }
    }
}
