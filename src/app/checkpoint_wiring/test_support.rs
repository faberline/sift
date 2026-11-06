//! Checkpoint fault adapters for application boundary tests.

use super::*;
use crate::journal::application::raft_checkpoint_service::CheckpointBarrierError;
use crate::journal::domain::checkpoint_port::JournalCheckpointPort;
use crate::journal::domain::raft_checkpoint_port::RaftCheckpointPort;
use crate::shared_kernel::archive_receipts::ExpirationReceipt;
use crate::shared_kernel::checkpoint_data::{CapturedPrefix, SnapshotCompactionOutcome};
use crate::shared_kernel::port_future::{PortFuture, PortValueFuture};
use crate::shared_kernel::retention_fence::RetentionFenceV1;
use crate::shared_kernel::sift_command::SiftCommandV1;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::sync::Mutex;

pub(crate) const BARRIER_BYTES: &[u8] = br#"{"kind":"archive_checkpoint_barrier","retention_generation":7,"manifest_uri":"gs://bucket/fixed/manifest.json","manifest_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BarrierFault {
    None,
    Encode,
    Propose,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum AllVoterResult {
    Installed,
    NotInstalled,
    Behind,
}

#[derive(Default)]
pub(crate) struct Probe {
    pub(crate) steps: Mutex<Vec<&'static str>>,
    pub(crate) proposals: Mutex<Vec<Vec<u8>>>,
}

impl Probe {
    fn enter(&self, step: &'static str) {
        self.steps.lock().unwrap().push(step);
    }
}

struct Raft {
    probe: Arc<Probe>,
    fault: BarrierFault,
    all_voters: AllVoterResult,
}

fn fault(context: &'static str) -> anyhow::Error {
    anyhow::Error::new(std::io::Error::other("fixed checkpoint fault")).context(context)
}

impl RaftCheckpointPort for Raft {
    type Error = anyhow::Error;

    fn encode(&self, command: &SiftCommandV1) -> Result<Vec<u8>> {
        self.probe.enter("encode");
        if self.fault == BarrierFault::Encode {
            return Err(fault("encode port"));
        }
        command.encoded()
    }

    fn propose(&self, command: Vec<u8>) -> PortFuture<'_, u64, Self::Error> {
        Box::pin(async move {
            self.probe.enter("propose");
            self.probe.proposals.lock().unwrap().push(command);
            if self.fault == BarrierFault::Propose {
                return Err(fault("propose port"));
            }
            Ok(41)
        })
    }

    fn is_leader(&self) -> PortValueFuture<'_, bool> {
        Box::pin(async { true })
    }

    fn require_snapshot_capability_on_all_voters(&self) -> PortFuture<'_, (), Self::Error> {
        Box::pin(async { Ok(()) })
    }

    fn require_applied_index_on_all_voters(&self, _: u64) -> PortFuture<'_, (), Self::Error> {
        Box::pin(async { Ok(()) })
    }

    fn snapshot_and_compact_through_outcome(
        &self,
        up_to: u64,
    ) -> PortFuture<'_, SnapshotCompactionOutcome, Self::Error> {
        Box::pin(async move {
            self.probe.enter("all voters");
            if self.all_voters == AllVoterResult::Behind {
                return Err(fault("voter is behind"));
            }
            Ok(SnapshotCompactionOutcome {
                snapshot_index: up_to,
                installed: self.all_voters == AllVoterResult::Installed,
            })
        })
    }

    fn snapshot_and_compact_through_quorum_outcome(
        &self,
        up_to: u64,
    ) -> PortFuture<'_, SnapshotCompactionOutcome, Self::Error> {
        Box::pin(async move {
            self.probe.enter("quorum");
            Ok(SnapshotCompactionOutcome {
                snapshot_index: up_to,
                installed: true,
            })
        })
    }
}

struct Checkpoints(Arc<Probe>);

impl JournalCheckpointPort for Checkpoints {
    type Error = anyhow::Error;

    fn pending_retention_fence(&self) -> Option<(RetentionFenceV1, u64)> {
        self.0.enter("pending fence");
        None
    }

    fn capture_archive_prefix(&self) -> Result<CapturedPrefix> {
        self.0.enter("capture");
        Ok((19, 0, Vec::new()))
    }

    fn expire_current_archive_at(&self, _: DateTime<Utc>) -> Result<Option<ExpirationReceipt>> {
        panic!("checkpoint tests do not expire events")
    }

    fn prepare_archive_checkpoint(&self, applied: u64, cursor: u64) -> Result<()> {
        assert_eq!((applied, cursor), (19, 0));
        self.0.enter("archive checkpoint");
        Ok(())
    }

    fn prepare_archive_checkpoint_without_gc(&self, applied: u64, cursor: u64) -> Result<()> {
        assert_eq!((applied, cursor), (19, 0));
        self.0.enter("no-GC checkpoint");
        Ok(())
    }

    fn prepare_local_checkpoint(&self, _: u64, _: u64) -> Result<()> {
        panic!("checkpoint tests use a remote archive")
    }

    fn prepare_resident_checkpoint(&self, _: u64, _: u64) -> Result<()> {
        panic!("checkpoint tests prepare the archive successfully")
    }
}

pub(crate) fn recording_raft(
    fault: BarrierFault,
    all_voters: AllVoterResult,
) -> (Arc<RaftCheckpointService>, Arc<Probe>) {
    let probe = Arc::new(Probe::default());
    let service = Arc::new(RaftCheckpointService::new(Arc::new(Raft {
        probe: probe.clone(),
        fault,
        all_voters,
    })));
    (service, probe)
}

pub(crate) fn lifecycle_context(
    all_voters: AllVoterResult,
) -> (tempfile::TempDir, LifecycleContext, Arc<Probe>) {
    let root = tempfile::tempdir().unwrap();
    let state = ServiceState::open(root.path()).unwrap();
    let mut ctx = state.lifecycle_context(Some("gs://bucket/fixed".into()));
    let (raft, probe) = recording_raft(BarrierFault::None, all_voters);
    ctx.raft = Some(raft);
    ctx.state_machine = Arc::new(JournalCheckpointService::new(Arc::new(Checkpoints(
        probe.clone(),
    ))));
    (root, ctx, probe)
}

#[tokio::test]
async fn barrier_use_case_keeps_native_bytes_and_error_chains() {
    for failure in [
        BarrierFault::None,
        BarrierFault::Encode,
        BarrierFault::Propose,
    ] {
        let (raft, probe) = recording_raft(failure, AllVoterResult::Installed);
        let result = raft
            .propose_archive_checkpoint_barrier(
                7,
                "gs://bucket/fixed/manifest.json".into(),
                "a".repeat(64),
            )
            .await;
        match result {
            Ok(index) => assert_eq!(index, 41),
            Err(error) => {
                let (source, expected) = match error {
                    CheckpointBarrierError::Encode(error) => (error, "encode port"),
                    CheckpointBarrierError::Propose(error) => (error, "propose port"),
                };
                assert_eq!(source.to_string(), expected);
                assert_eq!(source.root_cause().to_string(), "fixed checkpoint fault");
                assert!(source
                    .root_cause()
                    .downcast_ref::<std::io::Error>()
                    .is_some());
            }
        }
        if failure == BarrierFault::Encode {
            assert_eq!(*probe.steps.lock().unwrap(), ["encode"]);
            assert!(probe.proposals.lock().unwrap().is_empty());
        } else {
            assert_eq!(*probe.steps.lock().unwrap(), ["encode", "propose"]);
            assert_eq!(*probe.proposals.lock().unwrap(), [BARRIER_BYTES]);
        }
    }
}
