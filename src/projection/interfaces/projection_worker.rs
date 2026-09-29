//! The projection worker ServiceState starts, and its handle. The worker
//! keeps every projection caught up in the background; stopping it ends the
//! catch-up loop, then flushes the journal's dedupe window and persists every
//! projection.

use std::sync::Arc;

use chrono::Utc;

use crate::app::service_state::ServiceState;
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

impl ServiceState {
    /// Start the one in-process projection worker owned by the Sift service.
    /// The worker has no listener, WAL, or Raft group of its own and can be
    /// stopped after HTTP drain during graceful shutdown.
    pub fn start_projection_worker(&self) -> ProjectionWorker {
        let projections = self.projections.clone();
        let flush_projections = self.projections.clone();
        let journal = self.journal.clone();
        let flush_journal = self.journal.clone();
        let (shutdown, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                        let runtime = projections.clone();
                        let journal = journal.clone();
                        match tokio::task::spawn_blocking(move || {
                            journal.maintain_dedupe_at(Utc::now(), false)?;
                            journal.storage().seal_ready()?;
                            for name in runtime.projection_names() {
                                runtime.catch_up(&name)?;
                            }
                            anyhow::Ok(())
                        }).await {
                            Ok(Ok(_)) => {}
                            Ok(Err(error)) => tracing::warn!(%error, "projection worker catch-up failed"),
                            Err(error) => tracing::warn!(%error, "projection worker task panicked"),
                        }
                    }
                }
            }
        });
        ProjectionWorker {
            shutdown: Some(shutdown),
            task,
            projections: flush_projections,
            journal: flush_journal,
        }
    }
}
