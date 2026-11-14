# Sift version compatibility and rolling upgrade

This page covers 0.x releases of one Sift binary. The policy below is only what
the tests prove today. A claim with no proof is written as `not yet proven`.

## Compatibility policy

| Surface | Stays readable across 0.x | Proof |
|---------|---------------------------|-------|
| Event, manifest and query-job bytes | Decode and encode again to the same bytes. | `tests/fixtures/refactor_compatibility/{event-log,event-metric,event-span,manifest,query-job}.json`; tests `event_bytes_read_and_reencode_without_change`, `manifest_bytes_read_and_reencode_without_change` and `query_job_bytes_read_and_reencode_without_change` in `tests/it/refactor_compatibility.rs`. |
| Projection snapshots (log, metric, metric-v8, trace) | Restore the snapshot and produce the same bytes. | `tests/fixtures/refactor_compatibility/projection-*.json`; test `projection_snapshots_read_and_reencode_without_change` in `tests/it/refactor_compatibility.rs`. |
| Checkpoints `SIFTRCP1` (archive), `SIFTLCP1` (local) and `SIFTRSD1` (resident) | Decode and encode again to the same frame bytes, with the same refusal messages. | `tests/fixtures/refactor_compatibility/checkpoint-*.bin`; tests `archive_checkpoint_keeps_bytes_and_rejections`, `local_checkpoint_keeps_bytes_and_rejections` and `resident_checkpoint_keeps_bytes_and_rejections` in `src/journal/infrastructure/raft/checkpoint_compatibility_tests.rs`. |
| OpenAPI document and CLI help pages | Do not change by accident. | Tests `openapi_bytes_do_not_change` and `cli_help_bytes_do_not_change`; files `tests/fixtures/refactor_compatibility/openapi.json`, `openapi-020.json`, `cli-help.json` and `cli-help-020.json`. |
| Public Rust paths | Not a 0.x compatibility promise. Rust types, constructors and traits may change. | `docs/public-api.md` lists the public paths and says internal Rust types can change. |
| Full snapshot and Raft command formats | Each carries a format version: snapshot `SNAPSHOT_FORMAT_VERSION` 2 and command `COMMAND_FORMAT_VERSION` 2 in `src/journal/infrastructure/raft/`. A reader refuses an unknown version. | Checkpoint tests in `src/journal/infrastructure/raft/checkpoint_compatibility_tests.rs` and `tests/it/refactor_compatibility.rs`. |
| WAL segments written by an older release | Proven for the format in `tests/fixtures/wal-v1` (`WalBatch` `format_version` 1). | Tests `current_binary_opens_the_previous_format_wal` and `current_binary_keeps_writing_the_previous_wal_format` in `tests/it/previous_format_compat.rs`. No tagged release exists yet, so this fixture is the baseline of the first release. |
| Newer data format on an older binary | Refused with `unsupported Sift data format <n>; expected <m>` and no change on disk. | Test `newer_data_root_format_is_refused_and_left_unchanged` in `tests/it/previous_format_compat.rs`. |
| Mixed-version voters in one group | not yet proven | It needs two released versions and a live three-voter group. The check is a paid GKE rolling-upgrade drill that the user must approve; it is not part of this policy. |

The fixtures were captured from commit `5ae45b9` and must not be regenerated.
Formats carry versions. No migration step exists. A release that bumps a
version must keep a reader for the old version, or say so in its release note.

## Stability promise for 1.0.0

Draft: it applies only after the user confirms the 1.0.0 standard (faberline/aw issue 384).

| Surface | Version | Guarded by |
|---------|---------|------------|
| Public HTTP and gRPC API | OpenAPI document `tests/fixtures/refactor_compatibility/openapi.json` | Test `openapi_bytes_do_not_change` |
| CLI help and flags | `cli-help.json` | Test `cli_help_bytes_do_not_change` |
| Wire formats (OTLP, Prometheus Remote Write 1.0) | Protocol versions of the wire | not yet proven: no golden test pins these bytes |
| On-disk formats | Versions listed in the compatibility policy | Tests `newer_data_root_format_is_refused_and_left_unchanged` and `current_binary_keeps_writing_the_previous_wal_format` |
| `Sift` CRD | `k8s/crd/sift.yaml` | not yet proven: no test pins the CRD schema |

Rules:

- Versions follow semantic versioning. No breaking change to these surfaces
  before 2.0.0.
- A change that adds a field is allowed when old readers ignore it.
- Each surface has a test that fails when it changes (named in the table).
- Public Rust paths are not part of the promise (`docs/public-api.md`).

## Rolling upgrade

The store and control roles each use one fixed three-voter Raft group. Two
durable voters must commit a write before Sift reports success. Each pod that
owns local state has its own PVC at `/var/lib/sift` (`docs/operations/ha.md`).

Upgrade in this order: store, control, query and gateway Deployments, then the
agent DaemonSet. This is the policy of this document.

1. Take an online backup first:

   ```sh
   sift backup \
     --url http://<instance>.<namespace>.svc.cluster.local:7380 \
     --token-file /var/run/secrets/sift/client/token \
     --token-audience sift.axiom.dev \
     --project <instance> \
     --dest gs://example-sift-backups/sift \
     --retention-secs 604800
   ```

2. Upgrade the store group one voter at a time.
3. After each voter restarts on the new image, wait until
   `kubectl get pods -n <namespace>` shows it Ready and its `/readyz` returns
   200. Only then start the next voter.
4. Never take down a second store voter while one is not Ready. Two voters are
   needed for quorum.
5. Upgrade the control group the same way.
6. Upgrade the query and gateway Deployments, then the agent DaemonSet.

The one-at-a-time order is `not yet proven` by an automated test. The test is
issue #80. This repository does not document the operator's StatefulSet update
strategy, so the operator must confirm one-at-a-time behavior with
`kubectl rollout status statefulset/<instance>-store -n <namespace>`. That
behavior is `not yet proven` here.

## Rollback rule

Roll back one voter at a time, in the reverse order of the upgrade. Rollback is
allowed only while no persisted format of the new release has been written that
the old release cannot read.

Formats are versioned. Rollback of one voter is safe while no format version of
the new release has been written. The test
`current_binary_keeps_writing_the_previous_wal_format` proves that the current
release writes the previous WAL format. Once a newer format version is written,
the safe path is to stop the replacement process and restore the backup from
step 1:

```sh
sift restore \
  --data-dir /var/lib/sift \
  --source file:///recovery/sift-backup.json
```

A restore loses every write made after that backup.

## What is not covered

- The automated rolling-upgrade test (issue #80).
- A live GKE upgrade drill.
- Mixed-version voters in one group.
