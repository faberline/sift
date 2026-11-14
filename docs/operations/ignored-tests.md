# Ignored tests

These tests need pinned outside sources, so `cargo test` skips them. The CI job
`test-ignored` in `.github/workflows/ci.yml` prepares the sources with
`scripts/ci/prepare-ignored.sh` and runs them with
`cargo test --locked -p sift --test it -- --ignored --test-threads=1`. The job
pins the count (24) and fails when it changes.

| Test | File | Reason | CI job |
|------|------|--------|--------|
| `vat_managed_service_stdout_reaches_real_sift_query` | `tests/it/vat_service_observability_e2e.rs` | needs vat, lumen, tape, relay and defer installed on PATH | `test-ignored` |
| `vat_managed_lumen_stdout_reaches_real_sift_query` | `tests/it/vat_lumen_observability_e2e.rs` | needs vat and lumen installed on PATH | `test-ignored` |
| `fast_reconcile_passes_before_and_after_leader_takeover` | `tests/it/operator_reconcile_oracle.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `patch_receipt_must_prove_that_drift_was_applied` | `tests/it/operator_reconcile_oracle.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `reconcile_waits_for_the_same_object_and_a_newer_generation` | `tests/it/operator_reconcile_oracle.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `existing_cluster_failure_never_runs_terraform` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `explicit_bootstrap_keeps_its_creation_contract` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `valid_existing_cluster_is_read_only_and_has_exact_stdout` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `existing_cluster_rejects_wrong_identity_and_autopilot` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `run_reuse_path_is_existing_only_even_when_later_describe_fails` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `exact_run_vm_is_checked_twice_before_one_stop` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `forged_node_labels_and_foreign_group_members_cannot_cordon_or_stop` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `unreadable_identity_never_mutates_a_node_or_vm` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `a_valid_replacement_member_is_still_not_the_cordoned_instance` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `second_membership_read_failure_does_not_stop_the_vm` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `leader_change_during_the_load_wait_prevents_stop` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `removing_the_final_leader_check_reaches_an_invalid_failover_stop` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `stop_requires_an_applied_cordon_receipt` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `uncordon_can_restore_a_valid_repaired_run_member` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `removing_either_guard_is_detected_by_the_executed_operation_oracle` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `removing_existing_only_from_the_real_run_call_exposes_the_unsafe_fallback` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `patch_double_rejects_non_mutating_or_missing_field_incompatible_operations` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `uncordon_also_requires_fresh_pool_membership` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |
| `replacement_or_membership_drift_after_cordon_prevents_stop` | `tests/it/gke_operation_scope.rs` | needs FABERLINE_WORKSPACE: a faberline/workspace checkout | `test-ignored` |

No test moved to the normal suite: each one needs a pinned source or binary
from `scripts/ci/prepare-ignored.sh`.
