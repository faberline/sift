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
| Full snapshot and Raft command formats | not yet proven | `docs/public-api.md` says they keep separate framing. No format version or migration step exists. |
| WAL segments written by an older release | not yet proven | No test reads them. |
| Mixed-version voters in one group | not yet proven | No test runs them. |

The fixtures were captured from commit `5ae45b9` and must not be regenerated.
No format version or migration step exists. A release that changes a byte
format breaks this policy, and its release note must say so.

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

Format changes are not versioned (see the compatibility policy). So rollback
after a format-changing release is `not yet proven`. The safe path is to stop
the replacement process and restore the backup from step 1:

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
