use crate::app::journal_command_wiring::commit_context;
use crate::ingest::application::ingest_service::IngestService;
use crate::ingest::domain::batch_submission::{BatchSession, BatchSubmission};
use crate::ingest::infrastructure::capacity_adapter::NativeIngestCapacity;
use crate::journal::application::batch_limits::JournalBatchSizing;
use crate::journal::application::journal_service::DurableJournal;
use crate::journal::infrastructure::raft::sift_state_machine::SiftStateMachine;
use crate::node::infrastructure::local_capacity::LocalCapacity;
use crate::shared_kernel::port_future::PortFuture;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct DrainQueue {
    draining: Arc<AtomicBool>,
    calls: Mutex<Vec<String>>,
    fail_join: bool,
}

impl BatchSubmission for DrainQueue {
    type Error = anyhow::Error;
    fn begin(&self) -> anyhow::Result<Box<dyn BatchSession<Error = Self::Error>>> {
        unreachable!("drain must not begin an ingest session")
    }
    fn close(&self) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("close:{}", self.draining.load(Ordering::Acquire)));
    }
    fn join(&self) -> PortFuture<'_, (), Self::Error> {
        Box::pin(async move {
            self.calls.lock().unwrap().push("join".into());
            if self.fail_join {
                return Err(
                    anyhow::Error::new(std::io::Error::other("fixed join failure"))
                        .context("join accepted ingest requests"),
                );
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn ingest_service_sets_shared_drain_flag_before_close_and_keeps_join_failure_chain() {
    for fail_join in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let journal = Arc::new(DurableJournal::open(temp.path()).unwrap());
        let state_machine = Arc::new(SiftStateMachine::open(temp.path(), journal.clone()).unwrap());
        let capacity = Arc::new(LocalCapacity::open(temp.path(), 1 << 30, 1).unwrap());
        let context = commit_context(
            journal.clone(),
            None,
            state_machine,
            Arc::new(tokio::sync::Mutex::new(())),
            capacity.clone(),
        );
        let sizing = Arc::new(JournalBatchSizing::new(context.commands.clone()));
        let draining = Arc::new(AtomicBool::new(false));
        let queue = Arc::new(DrainQueue {
            draining: draining.clone(),
            calls: Mutex::new(Vec::new()),
            fail_join,
        });
        let service = IngestService::new(
            journal,
            draining.clone(),
            context,
            sizing,
            queue.clone(),
            Arc::new(NativeIngestCapacity { capacity }),
        );
        assert!(!service.is_draining());
        service.start_drain();
        assert!(draining.load(Ordering::Acquire));
        let result = service.finish_drain().await;
        if fail_join {
            assert_eq!(
                result
                    .unwrap_err()
                    .chain()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                ["join accepted ingest requests", "fixed join failure"]
            );
        } else {
            result.unwrap();
        }
        assert!(service.is_draining());
        assert_eq!(
            *queue.calls.lock().unwrap(),
            ["close:true", "close:true", "join"]
        );
    }
}
