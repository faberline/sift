#!/usr/bin/env python3
"""Run local Sift 0.2 capability checks with Python 3.11+ and print a JSON receipt."""

import hashlib
import json
import os
import platform
import re
import subprocess
import sys
import time
from pathlib import Path

import tomllib

GATE = Path(__file__).resolve()
ROOT = GATE.parents[1]
CLAIMS = {
    "bounded-log-projection": (
        "bounded_projections::logging_projection_deletes_evicted_documents_from_shared_text_index",
        "logging_store::retention_snapshot_and_restore_are_deterministic",
        "logging_store::logging_projection_rebuilds_equal_from_the_raw_journal",
    ),
    "bounded-metric-projection": (
        "bounded_projections::metric_projection_bounds_retained_points_without_event_id_identity",
        "bounded_projections::production_metric_projection_seals_chunks_and_bounds_its_memtable",
        "bounded_projections::late_metric_chunk_replacement_waits_for_checkpoint_and_reclaims_after_restart",
        "metric_store::metric_projection_rebuilds_equal_from_raw_events",
        "metric_store::broad_metric_query_materializes_only_limit_plus_one_matching_series",
    ),
    "bounded-trace-projection": (
        "bounded_projections::trace_projection_bounds_spans_and_dedupe_metadata_together",
        "trace_store::trace_projection_rebuilds_equal_from_raw_spans",
    ),
    "project-access-and-redaction": (
        "runtime_security_e2e::required_auth_protects_data_plane_but_not_operational_probes",
        "event_v2_governance::governance_redacts_and_truncates_before_raw_bytes_and_is_project_scoped",
        "kubernetes_auth::token_review_and_subject_access_review_bound_each_request_to_one_project",
        "kubernetes_auth::a_subject_access_review_deny_is_a_forbidden_response",
        "kubernetes_auth::global_backup_requires_a_namespace_admin_review",
        "kubernetes_auth::project_admin_cannot_write_control_records_in_another_project",
        "logging_store::identical_event_ids_are_isolated_by_project_in_records_and_text_index",
        "metric_store::identical_metric_event_ids_are_isolated_by_project",
        "trace_store::identical_span_event_ids_are_isolated_by_project",
    ),
    "cri-collector-recovery": (
        "collector_cri::cri_rewrite_detects_fast_regrowth_and_shortening_across_collector_restarts",
        "collector_cri::cri_file_reused_by_another_pod_keeps_both_resource_identities",
        "collector_cri::legacy_cri_checkpoint_preserves_its_offset_and_event_identity",
        "collector_cri::namespace_scoped_agent_does_not_collect_another_project_namespace",
        "collector_cri::cri_partial_rotation_trace_metadata_and_cloud_coexistence",
        "collector_cri::endpoint_outage_retains_offset_and_recovery_drains_file",
        "collector_cri::missing_observed_unread_source_is_durably_accounted",
        "collector_cri::follow_collector_survives_outage_without_process_restart",
        "collector_cri::follow_collector_stops_on_explicit_permanent_502",
    ),
    "shared-library-ownership": (
        "shared_library_graph::production_graph_points_from_sift_to_libs_only",
        "shared_library_graph::every_integration_test_source_is_compiled",
        "shared_library_manifest_aliases::manifest_uses_canonical_shared_library_names_without_aliases",
        "shared_runtime_structure::group_commit_is_owned_by_service_executor",
        "shared_runtime_structure::ingest_admission_mechanics_are_owned_by_service_http",
        "shared_runtime_structure::reverse_proxy_runtime_is_owned_by_service_http",
        "shared_runtime_structure::persistent_query_job_transitions_use_service_executor",
        "shared_runtime_structure::shutdown_order_and_task_failures_use_server_lifecycle",
        "shared_runtime_structure::scoped_bearer_middleware_is_owned_by_service_auth",
        "shared_runtime_structure::replicated_host_startup_is_owned_by_raft_runtime",
        "shared_runtime_structure::kubernetes_workloads_are_owned_by_service_k8s",
        "shared_runtime_structure::live_backup_http_transport_is_owned_by_service_backup",
        "shared_runtime_structure::logging_text_index_is_owned_by_index_text",
        "shared_runtime_structure::manifest_last_archive_flow_is_owned_by_storage_segment",
        "shared_runtime_structure::typed_projection_flow_is_owned_by_service_projection",
        "shared_runtime_structure::otlp_wire_and_direct_grpc_runtime_are_owned_by_transport_otlp",
        "collector_library_boundary::sift_collector_is_domain_hooks_over_service_collector",
        "mcp_library_boundary::sift_keeps_tools_while_service_mcp_owns_transports_and_security",
        "remote_write_library_boundary::sift_keeps_only_remote_write_domain_conversion",
    ),
    "baseline-ingest-and-restart-resilience": (
        "stability_e2e::bounded_ingest_burst_survives_drain_and_journal_reopen",
    ),
}


def capture(*args, env=None):
    result = subprocess.run(
        args, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, check=False
    )
    if result.returncode:
        sys.stderr.write(result.stdout[-20000:])
        result.check_returncode()
    return result.stdout


def source_state():
    revision = capture("git", "rev-parse", "HEAD").strip()
    path = str(GATE.relative_to(ROOT))
    committed = (
        capture("git", "ls-tree", "--name-only", revision, "--", path).strip() == path
    )
    status = capture(
        "git", "status", "--porcelain=v1", "--untracked-files=all"
    ).splitlines()
    # Allow the new gate to be tested before it is committed. Record that fact.
    if status != ([] if committed else [f"?? {path}"]):
        raise RuntimeError("the capability gate requires a clean product checkout")
    return revision, committed, hashlib.sha256(GATE.read_bytes()).hexdigest()


def core_ownership():
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    packages = [
        package
        for package in lock["package"]
        if package.get("source", "").startswith(
            "git+https://github.com/faberline/core?"
        )
    ]
    sources = {package["source"] for package in packages}
    if len(sources) != 1:
        raise RuntimeError(
            "the Sift lock must resolve one canonical faberline/core source"
        )
    source = sources.pop()
    revision = source.rsplit("#", 1)[-1]
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise RuntimeError(
            "the shared owner does not have an exact revision in Cargo.lock"
        )
    return {
        "repository": "faberline/core",
        "source": source,
        "source_commit": revision,
        "packages": sorted(package["name"] for package in packages),
    }


def main():
    start = time.monotonic()
    state = source_state()
    revision, gate_committed, gate_sha256 = state
    owners = core_ownership()
    cargo = os.environ.get("CARGO", "cargo")
    build = (
        cargo,
        "test",
        "--locked",
        "--package",
        "sift",
        "--test",
        "it",
        "--no-run",
        "--message-format=json-render-diagnostics",
    )
    output = capture(*build, env={**os.environ, "SIFT_SOURCE_REVISION": revision})
    executables = {}
    for line in output.splitlines():
        artifact = json.loads(line)
        if (
            artifact["reason"] == "compiler-artifact"
            and artifact.get("executable")
            and Path(artifact["manifest_path"]) == ROOT / "Cargo.toml"
        ):
            executables[artifact["target"]["name"]] = artifact["executable"]
    if not {"sift", "it"} <= executables.keys():
        raise RuntimeError(
            "Cargo did not report the Sift and integration test binaries"
        )
    info = json.loads(capture(executables["sift"], "acceptance-build-info"))
    if info.get("git_sha") != revision:
        raise RuntimeError("the Sift binary does not match the checked source")

    claims = []
    for claim, cases in CLAIMS.items():
        command = (
            executables["it"],
            "--exact",
            "--color",
            "never",
            "--test-threads=1",
            *cases,
        )
        output = capture(*command)
        passed = re.findall(r"^test (.+) \.\.\. ok$", output, re.MULTILINE)
        if not cases or sorted(passed) != sorted(cases):
            raise RuntimeError(f"{claim} did not pass every exact case\n{output}")
        claims.append(
            {"id": claim, "result": "passed", "passed": len(passed), "command": command}
        )
    if source_state() != state:
        raise RuntimeError("the source changed while running capability checks")
    print(
        json.dumps(
            {
                "schema": "sift.capability-evidence/v1",
                "result": "passed",
                "scope": "local",
                "production_accepted": False,
                "source_commit": revision,
                "gate_committed": gate_committed,
                "gate_sha256": gate_sha256,
                "sift_build": info,
                "environment": {
                    "platform": platform.platform(),
                    "cargo": capture(cargo, "--version").strip(),
                },
                "shared_owner": owners,
                "build_command": build,
                "claims": claims,
                "pending_claims": [
                    {
                        "id": "ingest-query-replay-soak",
                        "result": "pending",
                        "reason": "The rated GKE load gates require separate acceptance evidence.",
                    }
                ],
                "duration_seconds": round(time.monotonic() - start, 3),
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    try:
        main()
    except (
        KeyError,
        OSError,
        RuntimeError,
        ValueError,
        subprocess.CalledProcessError,
    ) as error:
        sys.exit(str(error))
