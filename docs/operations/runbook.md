# Sift data-plane runbook

This runbook is for the operator of a small Sift installation on GKE. Every
command below comes from `README.md`, `docs/operations/ha.md` or
`docs/production-v1.md`. Replace each `<namespace>`, `<instance>` and `<n>`
placeholder with your values. SLOs, incident process and on-call are later
work.

## Health and readiness

**Run**

```sh
kubectl get pods -n <namespace>
```

Then send `GET /readyz` to the public HTTPS endpoint of the installation.

**Healthy**

- Every gateway, query, store and control pod shows `Ready`. A small
  installation has 3 store pods and 3 control pods.
- `GET /readyz` returns HTTP 200.

Source: `docs/production-v1.md` (public HTTPS `/readyz`);
`src/operations/infrastructure/k8s/role_workloads.rs` (pod probes on
`/readyz` and `/healthz`).

## Metrics to watch

The collector exposes `/metrics` on port `9318`. It starts with
`sift collect --metrics-port 9318`. The generated GKE log agent exposes this
port. Its node-local metrics agent scrapes it every 30 seconds.

**Run**

Send `GET /metrics` to port `9318` of a `<instance>-agent` pod. The docs give
no separate command for this request.

**Healthy**

| Metric | Healthy | Source |
|---|---|---|
| `sift_collector_pending_records` | does not grow over time | `src/app/collector_metrics.rs` |
| `sift_collector_pending_metric_points` | does not grow over time | `src/app/collector_metrics.rs` |
| `sift_collection_lag_seconds` | falls after a restart and stays low | `src/app/collector_metrics.rs` |
| `sift_collector_lost_bytes_total` | stays at its value; does not grow | `src/app/collector_metrics.rs` |
| `sift_collector_lost_sources_total` | stays at its value; does not grow | `src/app/collector_metrics.rs` |
| `sift_local_storage_used_bytes` | below `sift_local_storage_max_bytes` | `src/app/interfaces/metrics_provider.rs` |
| `sift_local_storage_warning` | see the metric's source file | `src/app/interfaces/metrics_provider.rs` |
| `sift_local_storage_critical` | see the metric's source file | `src/app/interfaces/metrics_provider.rs` |
| `sift_remote_protection_lag_seconds` | see the metric's source file | `src/app/interfaces/metrics_provider.rs` |
| `sift_remote_protection_backpressure` | see the metric's source file | `src/app/interfaces/metrics_provider.rs` |
| `sift_query_failures_total` | does not grow | `src/app/interfaces/metrics_provider.rs` |
| `sift_notification_failures_total` | does not grow | `src/app/interfaces/metrics_provider.rs` |
| `sift_pending_notifications` | see the metric's source file | `src/app/interfaces/metrics_provider.rs` |
| `sift_control_leader` | see the metric's source file | `src/app/interfaces/metrics_provider.rs` |
| `sift_raw_events_total` | grows while sources send data | `src/app/interfaces/metrics_provider.rs` |
| `sift_duplicate_events_total` | see the metric's source file | `src/app/interfaces/metrics_provider.rs` |
| `sift_journal_fsync_total` | grows while data is written | `src/journal/interfaces/journal_metrics.rs` |

Source: `docs/production-v1.md` (collector metrics port and agent scrape);
the metric names are in the source files listed in the table.

## One store voter lost

Store and control each use one fixed three-voter Raft group. Two durable
voters must commit before a write succeeds. The groups do not support live
membership change. A voter count other than three is refused.

**Run**

```sh
kubectl get pods -n <namespace>
```

```sh
kubectl describe pod <instance>-store-<n> -n <namespace>
```

**Healthy**

- Two store pods stay `Ready`, and writes continue.
- The lost pod comes back `Ready` after the StatefulSet recreates it on its
  own PVC.

**Act**

- Let the StatefulSet recreate the pod. Each pod that owns local state has
  its own PVC at `/var/lib/sift`.
- Do not scale the store or change the voter count. The change is refused.
- If two voters are lost, writes stop. Go to the next section.

Source: `docs/operations/ha.md` ("Store and control each use one fixed
three-voter Raft group"; "Each pod that owns local state has its own PVC").

## Lost node or disk: restore from backup

Use this section when a node or disk is lost and the data must come back from
a backup.

**Run**

Take a live backup while the installation runs:

```sh
sift backup \
  --url http://<instance>.<namespace>.svc.cluster.local:7380 \
  --token-file /var/run/secrets/sift/client/token \
  --token-audience sift.axiom.dev \
  --project <instance> \
  --dest gs://example-sift-backups/sift \
  --retention-secs 604800
```

Restore an exact backup while the replacement Sift process is stopped:

```sh
sift restore \
  --data-dir /var/lib/sift \
  --source file:///recovery/sift-backup.json
```

For a whole-disk restore, use the recovery record. Save one
`recovery/latest.json` file first. Pass that same file to every offline
restore, and start each store and control data directory empty:

```sh
sift restore-complete --record-file recovery/latest.json
```

**Healthy**

- The restore command exits 0.
- After the replacement pod starts, `/readyz` returns HTTP 200.

**Act**

- `sift backup --data-dir` is offline only. Stop the process that owns the
  data root before you use it.
- `sift restore-complete` verifies both hashes and recovery positions. It
  keeps notification IDs and resets the control Raft position for a fresh
  fixed membership group.

Source: `docs/operations/ha.md` ("Backup and archive"); `docs/production-v1.md`
(whole-disk restore with `sift restore-complete --record-file`).

## Collector replay after restart

The collector replays unacknowledged input after a restart. Event IDs are
stable, so a replay does not create duplicates at Sift ingest. The checkpoint
lives under `/var/lib/sift/agent`.

**Run**

```sh
kubectl rollout restart daemonset/<instance>-agent -n <namespace>
```

**Healthy**

- `sift_collector_pending_records` returns to 0.
- `sift_collection_lag_seconds` falls.

**Act**

- A checkpoint advances only after Sift acknowledges a valid batch. A restart
  therefore replays only the batches that were not acknowledged.

Source: `docs/operations/structured-stdout.md` ("Collector guarantees");
`src/operations/infrastructure/k8s/metrics_agent.rs` (DaemonSet name
`<instance>-agent`); `src/operations/infrastructure/k8s/agent_workloads.rs`
(checkpoint path `/var/lib/sift/agent/checkpoint.json`).

## Rejected or missing source data

Rejected input goes to the quarantine file. On GKE the agent writes it to
`/var/lib/sift/agent/rejected.jsonl`. The default path is
`<checkpoint path>.rejected.jsonl`.

When retention removes a source that still has uncommitted bytes, the
collector records the loss. It increments `lost_bytes` and `lost_sources`
and writes a durable `source_lost` record. The counters survive a collector
restart.

**Run**

```sh
kubectl exec <agent pod> -n <namespace> -- tail -n 20 /var/lib/sift/agent/rejected.jsonl
```

Read `sift_collector_lost_bytes_total` on port `9318` of the agent pod, as in
the metrics section.

**Healthy**

- The quarantine file has no new lines.
- `sift_collector_lost_bytes_total` does not grow.

Source: `docs/production-v1.md` (quarantine file; loss counters survive
restart); `docs/operations/structured-stdout.md` ("Collector guarantees");
`src/bin/sift/collect.rs` (default quarantine path);
`src/operations/infrastructure/k8s/agent_workloads.rs` (GKE quarantine path).
