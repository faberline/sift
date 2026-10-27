//! The two barrier policies and the checkpoint permission used by real GC.

use super::lifecycle_checkpoint::{
    propose_no_gc_quorum_barrier, propose_retention_barrier, run_all_voter_checkpoint,
    run_no_gc_quorum_checkpoint,
};
use super::lifecycle_tick::TickFlags;
use super::run_lifecycle_attempt::LifecycleOutcome;
use crate::app::checkpoint_wiring::test_support::{
    lifecycle_context, recording_raft, AllVoterResult, BarrierFault, BARRIER_BYTES,
};

fn outcome() -> LifecycleOutcome {
    LifecycleOutcome {
        captured_applied_index: 19,
        pending_archive_gc: true,
        retention_generation: Some(7),
        manifest_uri: Some("gs://bucket/fixed/manifest.json".into()),
        manifest_sha256: Some("a".repeat(64)),
        ..Default::default()
    }
}

#[tokio::test]
async fn barrier_callers_keep_their_flags_and_stop_at_the_failed_port() {
    for no_gc in [false, true] {
        for initial in [false, true] {
            for fault in [
                BarrierFault::None,
                BarrierFault::Encode,
                BarrierFault::Propose,
            ] {
                let (raft, probe) = recording_raft(fault, AllVoterResult::Installed);
                let mut outcome = outcome();
                let mut allowed = initial;
                if no_gc {
                    propose_no_gc_quorum_barrier(&raft, &mut outcome, &mut allowed).await;
                } else {
                    propose_retention_barrier(&raft, &mut outcome, &mut allowed).await;
                }
                let success = fault == BarrierFault::None;
                assert_eq!(
                    outcome.captured_applied_index,
                    if success { 41 } else { 19 }
                );
                let expected = if no_gc {
                    success || initial
                } else {
                    success && initial
                };
                assert_eq!(allowed, expected);
                if fault == BarrierFault::Encode {
                    assert_eq!(*probe.steps.lock().unwrap(), ["encode"]);
                    assert!(probe.proposals.lock().unwrap().is_empty());
                } else {
                    assert_eq!(*probe.steps.lock().unwrap(), ["encode", "propose"]);
                    assert_eq!(*probe.proposals.lock().unwrap(), [BARRIER_BYTES]);
                }
            }
        }
    }
}

#[tokio::test]
async fn missing_barrier_identity_never_encodes_or_proposes() {
    for no_gc in [false, true] {
        for initial in [false, true] {
            for missing in 0..4 {
                let (raft, probe) = recording_raft(BarrierFault::None, AllVoterResult::Installed);
                let mut outcome = outcome();
                match missing {
                    0 => outcome.retention_generation = None,
                    1 => outcome.manifest_uri = None,
                    2 => outcome.manifest_sha256 = None,
                    _ => {
                        outcome.retention_generation = None;
                        outcome.manifest_uri = None;
                        outcome.manifest_sha256 = None;
                    }
                }
                let mut allowed = initial;
                if no_gc {
                    propose_no_gc_quorum_barrier(&raft, &mut outcome, &mut allowed).await;
                } else {
                    propose_retention_barrier(&raft, &mut outcome, &mut allowed).await;
                }
                assert_eq!(allowed, no_gc && initial);
                assert_eq!(outcome.captured_applied_index, 19);
                assert!(probe.steps.lock().unwrap().is_empty());
                assert!(probe.proposals.lock().unwrap().is_empty());
            }
        }
    }
}

#[tokio::test]
async fn behind_voter_and_uninstalled_checkpoint_keep_archive_gc_blocked() {
    for all_voters in [
        AllVoterResult::Installed,
        AllVoterResult::NotInstalled,
        AllVoterResult::Behind,
    ] {
        let (_root, ctx, probe) = lifecycle_context(all_voters);
        let mut flags = TickFlags {
            changed: false,
            replicated_checkpoint_installed: false,
        };
        run_all_voter_checkpoint(
            &ctx,
            ctx.raft.as_ref().unwrap(),
            &outcome(),
            true,
            &mut flags,
        )
        .await;
        let installed = all_voters == AllVoterResult::Installed;
        assert_eq!(flags.replicated_checkpoint_installed, installed);
        assert_eq!(flags.changed, all_voters != AllVoterResult::NotInstalled);
        assert_eq!(flags.allows_archive_gc(true, true, false, true), installed);
        assert!(!flags.allows_archive_gc(false, true, false, true));
        assert!(!flags.allows_archive_gc(true, false, false, true));
        assert!(!flags.allows_archive_gc(true, true, true, true));
        assert!(flags.allows_archive_gc(true, true, false, false));
        let expected: &[&str] = match all_voters {
            AllVoterResult::Installed => &[
                "capture",
                "archive checkpoint",
                "all voters",
                "pending fence",
            ],
            AllVoterResult::NotInstalled => &["capture", "archive checkpoint", "all voters"],
            AllVoterResult::Behind => &[
                "capture",
                "archive checkpoint",
                "all voters",
                "capture",
                "no-GC checkpoint",
                "quorum",
                "pending fence",
            ],
        };
        assert_eq!(*probe.steps.lock().unwrap(), expected);
    }
}

#[tokio::test]
async fn quorum_without_gc_changes_state_without_granting_gc_permission() {
    let (_root, ctx, probe) = lifecycle_context(AllVoterResult::Behind);
    let mut flags = TickFlags {
        changed: false,
        replicated_checkpoint_installed: false,
    };
    run_no_gc_quorum_checkpoint(&ctx, ctx.raft.as_ref().unwrap(), &outcome(), &mut flags).await;
    assert!(flags.changed);
    assert!(!flags.replicated_checkpoint_installed);
    assert!(!flags.allows_archive_gc(true, true, false, true));
    assert_eq!(
        *probe.steps.lock().unwrap(),
        ["capture", "no-GC checkpoint", "quorum", "pending fence"]
    );
}
