//! The archive worker ServiceState starts: the lifecycle loop, which runs a
//! tick on each interval while this node leads, and the handle that stops it.

use crate::app::service_state::ServiceState;
use crate::archive::application::lifecycle_tick::run_lifecycle_tick;
use crate::archive::application::run_lifecycle_attempt::LifecycleContext;

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

    fn start_lifecycle_worker(
        &self,
        destination: Option<String>,
        interval: std::time::Duration,
    ) -> ArchiveWorker {
        let ctx = LifecycleContext {
            journal: self.journal.clone(),
            raft: self.raft.clone(),
            state_machine: self.state_machine.clone(),
            local_capacity: self.local_capacity.clone(),
            destination,
        };
        let (shutdown, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(async move {
            loop {
                let leader = match &ctx.raft {
                    Some(raft) => raft.is_leader().await,
                    None => true,
                };
                if leader {
                    run_lifecycle_tick(&ctx).await;
                }

                tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(interval.max(std::time::Duration::from_millis(1))) => {}
                }
            }
        });
        ArchiveWorker {
            shutdown: Some(shutdown),
            task,
        }
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
