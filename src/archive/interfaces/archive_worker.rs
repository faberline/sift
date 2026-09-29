//! The archive worker handle ServiceState starts, and stopping it.

use crate::app::service_state::ServiceState;

impl ServiceState {
    /// Start the store lifecycle worker. A replicated follower stays idle.
    /// The worker records the remote manifest before it permits WAL compaction.
    pub fn start_archive_worker(
        &self,
        destination: impl Into<String>,
        interval: std::time::Duration,
    ) -> ArchiveWorker {
        self.start_lifecycle_worker(Some(destination.into()), interval)
    }

    /// Start the same leader-only lifecycle worker for an installation that
    /// has no remote archive. It commits the local immutable segment set before
    /// it compacts WAL bytes.
    pub fn start_local_archive_worker(&self, interval: std::time::Duration) -> ArchiveWorker {
        self.start_lifecycle_worker(None, interval)
    }
}

pub struct ArchiveWorker {
    pub(super) shutdown: Option<tokio::sync::watch::Sender<bool>>,
    pub(super) task: tokio::task::JoinHandle<()>,
}

impl ArchiveWorker {
    pub async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(true);
        }
        let _ = self.task.await;
    }
}
