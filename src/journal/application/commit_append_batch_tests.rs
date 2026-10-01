//! Fault ports prove the capacity and durable acknowledgement boundaries.

use super::commit_append_batch::CommitContext;
use crate::journal::domain::command_submission::{
    CapacityReservation, CommandSubmission, JournalCapacity, JournalReceiptLookup,
};
use crate::shared_kernel::append_result::AppendResult;
use crate::shared_kernel::event::EventEnvelope;
use crate::shared_kernel::port_future::PortFuture;
use crate::shared_kernel::sift_command::SiftCommandV1;
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};

type Calls = Arc<Mutex<Vec<&'static str>>>;
struct Commands {
    calls: Calls,
    fail: bool,
}
impl CommandSubmission for Commands {
    type Error = anyhow::Error;
    fn applied_commit_index(&self) -> u64 {
        self.calls.lock().unwrap().push("index");
        7
    }
    fn take_append_outcomes(&self, _: u64) -> Option<Vec<AppendResult>> {
        self.calls.lock().unwrap().push("outcomes");
        Some(vec![outcome(false, 8)])
    }
    fn append_size_bound(&self, _: Vec<EventEnvelope>) -> Result<usize> {
        Ok(1)
    }
    fn commit_command(&self, _: SiftCommandV1) -> PortFuture<'_, u64, Self::Error> {
        Box::pin(async move {
            self.calls.lock().unwrap().push("submit");
            if self.fail {
                bail!("proposal offline")
            }
            Ok(8)
        })
    }
}
struct Receipts {
    calls: Calls,
    duplicate: bool,
}
impl JournalReceiptLookup for Receipts {
    type Error = anyhow::Error;
    fn result_for_at(&self, _: &str, _: &str, _: DateTime<Utc>) -> Result<Option<AppendResult>> {
        self.calls.lock().unwrap().push("lookup");
        Ok(self.duplicate.then(|| outcome(true, 1)))
    }
}
struct Capacity {
    calls: Calls,
    fail: bool,
}
struct Lease {
    calls: Calls,
    committed: bool,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if !self.committed {
            self.calls.lock().unwrap().push("release");
        }
    }
}
impl CapacityReservation for Lease {
    fn commit(mut self: Box<Self>) {
        self.committed = true;
        self.calls.lock().unwrap().push("commit");
    }
}
impl JournalCapacity for Capacity {
    type Error = anyhow::Error;
    fn reserve(&self, _: u64) -> Result<Box<dyn CapacityReservation + '_>> {
        self.calls.lock().unwrap().push("reserve");
        if self.fail {
            bail!("disk full")
        }
        Ok(Box::new(Lease {
            calls: self.calls.clone(),
            committed: false,
        }))
    }
}
fn outcome(duplicate: bool, commit_index: u64) -> AppendResult {
    AppendResult {
        event_id: "event".into(),
        acknowledged_at: "2026-10-01T00:00:00Z".into(),
        cursor: 1,
        raw_cursor: 1,
        commit_index,
        duplicate,
    }
}
fn event() -> EventEnvelope {
    serde_json::from_str::<crate::shared_kernel::stored_event::StoredEvent>(include_str!(
        "../../../tests/fixtures/refactor_compatibility/event-log.json"
    ))
    .unwrap()
    .event
}
fn context(duplicate: bool, capacity_fail: bool, command_fail: bool) -> (CommitContext, Calls) {
    let calls = Calls::default();
    (
        CommitContext {
            journal: Arc::new(Receipts {
                calls: calls.clone(),
                duplicate,
            }),
            commands: Arc::new(Commands {
                calls: calls.clone(),
                fail: command_fail,
            }),
            local_capacity: Arc::new(Capacity {
                calls: calls.clone(),
                fail: capacity_fail,
            }),
        },
        calls,
    )
}

#[tokio::test]
async fn reservation_failure_precedes_receipt_and_proposal_work() {
    let (context, calls) = context(false, true, false);
    let error = context
        .append_governed_batch(vec![event()])
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "reserve local WAL and segment capacity before Raft admission"
    );
    assert_eq!(error.root_cause().to_string(), "disk full");
    assert_eq!(*calls.lock().unwrap(), ["reserve"]);
}
#[tokio::test]
async fn duplicate_reply_releases_capacity_without_a_command() {
    let (context, calls) = context(true, false, false);
    let results = context.append_governed_batch(vec![event()]).await.unwrap();
    assert_eq!(results[0].commit_index, 7);
    assert!(results[0].duplicate);
    assert_eq!(
        *calls.lock().unwrap(),
        ["reserve", "index", "lookup", "release"]
    );
}
#[tokio::test]
async fn proposal_failure_releases_capacity_and_never_acknowledges() {
    let (context, calls) = context(false, false, true);
    let error = context
        .append_governed_batch(vec![event()])
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "proposal offline");
    assert_eq!(
        *calls.lock().unwrap(),
        ["reserve", "index", "lookup", "submit", "release"]
    );
}
#[tokio::test]
async fn only_durable_outcomes_commit_the_capacity_reservation() {
    let (context, calls) = context(false, false, false);
    let results = context.append_governed_batch(vec![event()]).await.unwrap();
    assert_eq!(results[0].commit_index, 8);
    assert_eq!(
        *calls.lock().unwrap(),
        ["reserve", "index", "lookup", "submit", "outcomes", "commit"]
    );
}
