# Sift version compatibility and rolling upgrade

This page covers the 0.x releases of one Sift binary. The policy below states
only what the tests prove today. A claim with no proof is written as
`not yet proven`.

## Compatibility policy

| Surface | Stays readable across 0.x | Proof |
|---|---|---|
| Event, manifest and query-job bytes | Decode and re-encode to the same bytes. | `tests/fixtures/refactor_compatibility/{event-log,event-metric,event-span,manifest,query-job}.json`; tests `event_bytes_read_and_reencode_without_change`, `manifest_bytes_read_and_reencode_without_change`, `query_job_bytes_read_and_reencode_without_change` in `tests/it/refactor_compatibility.rs`. |
| Projection snapshots (log, metric, metric-v8, trace) | Decode and re-encode to the same bytes. | `tests/fixtures/refactor_compatibility/projection-*.json`; test `projection_snapshots_read_and_reencode_without_change` in `tests/it/refactor_compatibility.rs`. |
| Checkpoints `SIFTRCP1` (archive), `SIFTLCP1` (local), `SIFTRSD1` (resident) | Keep their bytes and their rejections. | `tests/fixtures/refactor_compatibility/checkpoint-*.bin`; tests `archive_checkpoint_keeps_bytes_and_rejections`, `local_checkpoint_keeps_bytes_and_rejections`, `resident_checkpoint_keeps_bytes_and_rejections` in `src/journal/infrastructure/raft/checkpoint_compatibility_tests.rs`. |
| OpenAPI document and CLI help pages | Do not change by accident. | Tests `openapi_bytes_do_not_change` and `cli_help_bytes_do_not_change` in `tests/it/refactor_compatibility.rs`; files `openapi.json`, `openapi-020.json`, `cli-help.json`, `cli-help-020.json` in `tests/fixtures/refactor_compatibility/`. |
| Public Rust paths | Not a 0.x promise. Rust types, constructors and traits may change. | `docs/public-api.md` lists the public paths and states that internal Rust types can change. |
| Full snapshot and Raft command formats | `not yet proven`. | `docs/public-api.md` says these keep their separate framing. No format version or migration step exists. |
| WAL segments written by an older release | `not yet proven`. | None. |
| Mixed-version voters in one group | `not yet proven`. | None. |

The fixtures were captured from `5ae45b9079538af8f70a5bfd4f3c0948adff83a6`.
Do not regenerate them. No format version or migration step exists. A release
that changes a byte format breaks this policy, and its release note must say so.

## Rolling upgrade

The store and control roles each use one fixed three-voter Raft group. Two
durable voters must commit a write before Sift reports success. Live membership
changes are refused. Each pod has its own PVC at `/var/lib/sift`.

Upgrade in this order. Wait for each step to finish before the next one starts.

1. Take an online backup. Use the `sift backup` command in the section
   "Backup and archive" of `docs/operations/ha.md`.
2. Upgrade the store group first, one voter at a time.
3. After each store voter restarts on the new image, wait until
   `kubectl get pods -n <namespace>` shows it Ready and `GET /readyz` returns
   HTTP 200. Then start the next voter.
4. Never stop a second voter while another voter is not Ready. Two voters are
   needed for quorum.
5. Upgrade the control group the same way.
6. Upgrade the query and gateway Deployments.
7. Upgrade the agent DaemonSet.

The `<instance>-store` StatefulSet update strategy is not documented in this
repository. The operator must confirm one-at-a-time behavior with
`kubectl rollout status statefulset/<instance>-store -n <namespace>`. The
one-at-a-time behavior of the operator's update is `not yet proven`.

## Rollback rule

Roll back one voter at a time, in the reverse order of the upgrade. A rollback
is allowed only while the new release has written no persisted format that the
old release cannot read.

Format changes are not versioned, so a rollback after a format-changing release
is `not yet proven`. The safe path is a restore from the backup taken in step 1.
Use `sift restore` as shown in the section "Backup and archive" of
`docs/operations/ha.md`. A restore loses the writes made after the backup.

## What is not covered

- The automated rolling-upgrade test (issue #80).
- A live GKE upgrade drill.
- Mixed-version voters in one group.
