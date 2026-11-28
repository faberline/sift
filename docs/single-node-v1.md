# Single-node profile contract (single-node-v1)

## Scope

The profile is one `all`-role Pod on one PVC on Standard GKE, installed by the operator with `topology: singleNode` (issues #220 to #222 of #214). It is a small-volume profile for one team. The acceptance run waits for #218 and #220 to #231.

[What holds the memory of one process, and whether it goes down after ingest stops](operations/single-node-memory.md)

## Promises

- One `all`-role Pod on one PVC on Standard GKE.
- It collects the container logs and the Prometheus metrics of the selected namespaces.
- It answers `sift_query` and PromQL.
- Retention is set by the `retentionDays` setting (R5).
- Logs of a Pod that is deleted soon after exit are kept (R2).

## Does not promise

- No quorum and no failover.
- Recovery only from the PVC, or from GCS when a `gs://` destination is set.
- No three-voter availability claim.
- A new Pod that starts while the old Pod still holds the data root lock does not wait. Its start fails with the error "another Sift process may be using it" and the process exits with an error.

## Acceptance gates

Every gate is **not run**. No live GKE evidence exists yet.

1. **G1** One documented install on Standard GKE with a single node, no GCS and no FQDN policy, that collects the container logs and the Prometheus metrics of one namespace and answers `sift_query` and PromQL for them. The acceptance phase `install_one_namespace` (`scripts/single_node/acceptance/phases.py`) looks for the log records and the metric series of the namespace and sends one `sift_query` call and one PromQL query; its result `install_one_namespace.json` has `"gate": "G1"`; it does not prove an install with more than one node or namespace, GCS, an FQDN policy or query speed.
2. **G2** Retention set to 30 days is enforced. G2 has two proofs. (a) The acceptance phase `retention` (`scripts/single_node/acceptance/phases.py`) sets `retentionDays` to 1 and proves that the setting reaches the Pod (`--retention-days 1`), that a batch older than the retention is rejected at ingest and is not queryable, and that a batch inside it is accepted and queryable. Its result `retention.json` has `"gate": "G2"`, `"proves"` and `"does_not_prove"`; it does not prove expiry of stored data, a retention of 30 days or reuse of disk. (b) The expiry of stored data without GCS is proved by the test `local_worker_expires_old_events_with_an_injected_clock` in `tests/archive_worker.rs`, which runs the lifecycle worker with an injected clock. G2 is complete only when both proofs exist.
3. **G3** Logs of a Pod that is deleted within 10 s of exit are queryable. The acceptance phase `deleted_pod_logs` (`scripts/single_node/acceptance/phases.py`) starts a Pod that writes one marker line and exits, deletes it within 10 s, and looks for the marker with the Pod name and namespace; its result `deleted_pod_logs.json` has `"gate": "G3"`; it does not prove a Pod deleted sooner than 10 s or many Pods at once.
4. **G4** The functional and PromQL-oracle gates that already run in the CI workflow `.github/workflows/ci.yml` with the default `serve --role all` (the default of `--role` in `src/bin/sift/serve_args.rs` is `all`):
   - `test: compat`: its step `prometheus compliance` runs `tests/prometheus_compliance.sh`, which starts `sift serve` with the default role and compares PromQL answers with the oracle.
   - `test: perf (metric workload)`: its step `metric workload` runs `tests/perf/metric_workload.sh`, which starts `sift serve` with the default role and runs the metric workload.

   These gates run on a CI runner, not on GKE.

### Phases and gates

| Phase or check | Gate | Note |
|---|---|---|
| `setup` | none | the install that G1 uses; G1 is proved by `install_one_namespace`; `setup.json` has no `gate` field |
| `install_one_namespace` | G1 | result has `"gate": "G1"` |
| `load_30m` | none | load evidence; no single gate |
| `load_24h` | none | load evidence; no single gate |
| `faults` | none | restart evidence; no single gate |
| `comparison` | none | read-back evidence; no single gate |
| `retention` | G2 | result has `"gate": "G2"` (proof a of G2) |
| `deleted_pod_logs` | G3 | result has `"gate": "G3"` |

Every gate stays **not run**. G4 runs in the CI workflow, not in the acceptance run.

The load phase named load_24h in the single-node acceptance runs 7080 seconds at 50 items per second, not 24 hours. The cost page docs/operations/single-node-acceptance-cost.md gives the figure.

## Senders

**Basis.** Read from the code of this repository, not measured, not a GKE claim. No live GKE evidence exists yet.

**Agents and nodes.** One log agent and one metric agent run for each node (`src/operations/infrastructure/k8s/agent_workloads.rs`, `src/operations/infrastructure/k8s/metrics_agent.rs`). The log agent is the DaemonSet of `agent_daemon_set_plan` in `src/operations/infrastructure/k8s/agent_workloads.rs`; it reads `/var/log/pods` of its own node. The metric agent is a second DaemonSet, named `<name>-agent-metrics` (`src/operations/infrastructure/k8s/metrics_agent.rs`). The log agent requests 25m CPU and 64Mi memory and is limited to 500m CPU and 256Mi memory (`src/operations/infrastructure/k8s/agent_workloads.rs`). Its data path is `/var/lib/sift/agent` (same file). With N nodes there are 2N agent Pods (`src/operations/infrastructure/k8s/agent_workloads.rs`, `src/operations/infrastructure/k8s/metrics_agent.rs`), and each one reads or scrapes only its own node.

**Cost for each Pod.** The log agent keeps one checkpoint entry for each log file, in one JSON file, `/var/lib/sift/agent/checkpoint.json` (`src/operations/infrastructure/k8s/agent_workloads.rs`). The entry is keyed by device, inode and birth time, and has the fields `offset`, `line`, `observed_len`, `relative_path`, `workload`, `retired`, `loss_reported`, `generation`, `anchor` and `birth_nanos` (`src/collector/infrastructure/cri_checkpoint.rs`). The code gives no byte size for an entry: bytes for each entry are not measured, and no constant for them exists in the code. Entries of removed files stay in the map with `retired: true`; the code that sets `file.retired = true` (`src/collector/infrastructure/cri_source/state.rs`) does not delete them. A node with many short-lived Pods therefore grows the checkpoint file; no rate is claimed. CRI discovery reads the directory tree under the root to a depth of 3 and skips symlinks (`src/collector/infrastructure/cri_discovery.rs`). The metric agent keeps no state for each Pod except the queue below.

**Store down: logs.** The agent runs with `--follow` (`src/operations/infrastructure/k8s/agent_workloads.rs`), so its delivery retry mode is `DeliveryRetryMode::UntilCancelled` (`src/collector/application/runtime.rs`): it retries a failed delivery without limit and does not read past the batch. It holds one pending batch on disk, a JSON file written with fsync before each send, of at most 1000 events and 64 MiB (`src/collector/infrastructure/durable_delivery.rs`). A bigger batch is refused as a permanent failure. After that one batch the agent stops reading. It drops nothing itself: the CRI source keeps the file offset in the checkpoint, and the log lines stay in the node log files (`src/collector/infrastructure/cri_source/state.rs`). What the node does with its log files while the agent waits (rotation) is the kubelet setting; it is not in this code, and no number is given for it.

**Store down: metrics.** The metric agent scrapes the Pods of its node one after another. One scrape of one Pod is split into batches of 1000 samples (`src/collector/infrastructure/metric_scraper.rs`), and the batches are saved to `pending-metrics.json` before the first send, at most 64 MiB (`src/collector/infrastructure/metric_queue.rs`). While the store is down, it stays on the current Pod and retries every `min(interval, 30 s)` (`src/collector/infrastructure/metric_scraper.rs`); the interval defaults to 30 s (`src/bin/sift/scrape.rs`). It scrapes no other Pod in that time. It drops nothing, but the Pods that wait lose their scrape intervals: no sample is made for the time of the wait. One scrape response is at most 8 MiB (`src/collector/infrastructure/metric_scraper.rs`). The scrape port defaults to 9090 and the path to `/metrics` (`src/collector/infrastructure/metric_scraper.rs`).

**Retry policy of the sender client.**

- One-shot mode retries 3 times by default (`DEFAULT_MAX_RETRIES` in `src/collector/domain/config.rs`; the config allows at most 20), and each wait is capped at 5 s (`src/collector/application/runtime.rs`).
- It retries HTTP 429, every 5xx status and a transport error. All other statuses and a partial success are permanent (`src/collector/infrastructure/client.rs`).
- The request timeout is 10 s by default (`--request-timeout-secs`, `src/bin/sift/collect.rs`).
- The batch size is 100 by default and at most 1000; the line limit is 512 KiB by default and at most 1 MiB (`src/collector/domain/config.rs`).

**Hundreds of Pods in several namespaces: is anything needed beyond the namespace lists?** **Yes**, with this evidence:

- The log agent render passes the list `spec.logs.namespaces` (`log_namespaces(cx, spec)` in `src/operations/infrastructure/k8s/agent_workloads.rs`); an empty list keeps the namespace of the `Sift` object. The metric agent render still passes one namespace, `cx.ns()` (`src/operations/infrastructure/k8s/metrics_agent.rs`); #227 adds the list. Both agents already take a comma-separated list (`value_delimiter = ','` in `src/bin/sift/collect.rs` and `src/bin/sift/scrape.rs`).
- The metric agent scrapes the Pods of its node one after another (`src/collector/infrastructure/metric_scraper.rs`), so one round takes the sum of the Pod scrape times. For many Pods on one node that sum can exceed the 30 s interval. This follows from the loop; no number is given.
- A Pod needs both the annotation `prometheus.io/scrape` = `true` and the label `sift.axiom.dev/scrape` = `true` (`src/collector/infrastructure/metric_scraper.rs`), so a namespace list alone does not select Pods.

This issue makes no change to the code.

## Capacity

The numbers come from `docs/operations/single-node-capacity-result.json` (run 44511577882 of the workflow `sift-single-node-load`; each step lasted 180 s). The run was measured on linux, 4 CPUs, 16758398976 bytes of memory, ext4, not a GKE claim. An item is one item that `accepted_items` counts in `scripts/single_node/run_local_load.py`.

| Offered items per second | Accepted items | Rejected items | Ingest p95 (s) | Log query p95 (s) | Metric query p95 (s) | Peak RSS (GB) | Disk bytes per accepted item |
|---|---|---|---|---|---|---|---|
| 500 | 90000 | 0 | 0.05 | 0.08 | 0.04 | 1.4 | 2326 |
| 1000 | 180000 | 0 | 0.11 | 0.26 | 0.09 | 5.2 | 4275 |
| 2500 | 450000 | 0 | 0.52 | 6.14 | 0.46 | 7.2 | 2056 |
| 5000 | 900000 | 0 | 1.37 | 10.08 | 3.03 | 8.6 | 2277 |
| 10000 | 1089000 | 711000 | 1.67 | 12.62 | 8.81 | 11.3 | 2462 |

The measured limit is 5000 items per second: the highest step with 0 rejected and 0 error items. At 10000 offered the run accepted 1089000 and rejected 711000 items.

The data root grew by 2056 to 4275 bytes for each accepted item.

Use this formula to size the PVC:

```
PVC bytes = items per second x 86400 x retentionDays x 4275 x 1.3
```

86400 is the number of seconds in a day. `retentionDays` is the setting `spec.retentionDays` (an integer from 1 to 180, default 180; see `k8s/crd/sift.yaml` and `docs/operations/configuration.md`). 4275 is the highest measured number of bytes for each item. 1.3 is the 30 percent margin. Worked example: 100 items per second for 30 days needs about 1441 GB. The disk bytes are `disk_bytes_added` of the whole data root; this section makes no claim for more than that.

The rates for each Pod (1, 5 and 20 items per second) are an assumption, the owner has not given the real log volume. Fit means ingest only: the offered rate is at most 70 percent of the measured limit (3500 items per second).

| Pods | Items per second for each Pod | Items per second in all | Fit |
|---|---|---|---|
| 100 | 1 | 100 | fit |
| 100 | 5 | 500 | fit |
| 100 | 20 | 2000 | fit |
| 300 | 1 | 300 | fit |
| 300 | 5 | 1500 | fit |
| 300 | 20 | 6000 | not fit |
| 500 | 1 | 500 | fit |
| 500 | 5 | 2500 | fit |
| 500 | 20 | 10000 | not fit |

- Fit does not mean a good query: at 2500 items per second the log query p95 is 6.14 s (#279) and the peak RSS is 7.2 GB on a 16 GB machine. What holds the memory: #280, #281, #282, #283.
- Not measured: GKE, a run longer than 180 s for each step, and memory growth over time.

Above the measured limit, or when the profile needs quorum and failover, use the three-voter topology in [production-v1.md](production-v1.md).

## Log query p95 under ingest load

The first table comes from run 44737037877 of the workflow `sift-single-node-load` (main d7e68f58, defaults, each step lasted 180 s; artifact `single-node-load-result/result.json`). The second table comes from run 44511577882, the run of the [Capacity](#capacity) section. Both runs were measured on a GitHub runner with 4 CPUs, not a GKE claim.

Run 44737037877:

| Offered items per second | Ingest p95 (s) | Log query p95 (s) | Query errors |
|---|---|---|---|
| 500 | 0.05 | 0.13 | 0 |
| 1000 | 0.11 | 1.24 | 0 |
| 2500 | 0.35 | 8.28 | 0 |
| 5000 | 0.84 | 13.78 | 1 |
| 10000 | 1.47 | 13.53 | 7 |

Run 44511577882:

| Offered items per second | Ingest p95 (s) | Log query p95 (s) | Query errors |
|---|---|---|---|
| 500 | 0.05 | 0.08 | 0 |
| 1000 | 0.11 | 0.26 | 0 |
| 2500 | 0.52 | 6.14 | 0 |
| 5000 | 1.37 | 10.08 | 0 |
| 10000 | 1.67 | 12.62 | 8 |

The highest offered rate at which the log query p95 stays below 1 second is 500 items per second (0.13 s, run 44737037877). At 1000 items per second the log query p95 was 1.24 s in run 44737037877 and 0.26 s in run 44511577882, so the two runs do not agree at that step.

At 10000 items per second the first run accepted 961000 of 1800000 offered items and rejected 839000; this step is an overload.

GitHub runner with 4 CPUs, not a GKE claim.

## Memory

The numbers come from `docs/operations/single-node-memory-result.json` (run 44737055221 of the workflow `sift-single-node-load` on main d7e68f58, soak mode: 1000 items per second for 600 s, then 120 s with no ingest). The run was measured on linux, 4 CPUs, 16759431168 bytes of memory, ext4, not a GKE claim.

| Measure | Bytes | GB |
|---|---|---|
| Peak RSS of the run (`peak_rss_bytes`) | 7469522944 | 7.5 |
| Highest sample during the soak (every 30 s) | 5347778560 | 5.3 |
| RSS at 600 s, the end of ingest | 5093838848 | 5.1 |
| RSS after 120 s with no ingest (`rss_after_quiet_bytes`) | 5093838848 | 5.1 |

At 1000 items per second the RSS grew from 521433088 bytes at 30 s to 5347778560 bytes at 510 s, and did not go down in the quiet time.

Give the Pod at least this memory at 1000 items per second: peak RSS x 1.3 = 9.7 GB. 1.3 is the 30 percent margin. This is the figure of one run on one machine shape. The memory limit of the Pod must be at least this figure.

Set it with `spec.resources`: for example `spec.resources.all.limits.memory` and `spec.resources.all.requests.memory` (field reference: [operations/configuration.md](operations/configuration.md)). The default request is 256Mi and there is no limit.

- Not measured: other rates, a run longer than 600 s, GKE, and the memory of a Pod that runs the log agent.
- What holds the memory and why it does not go down: [single-node-memory.md](operations/single-node-memory.md).

## Relation to production-v1

[production-v1.md](production-v1.md) and its 13-gate acceptance are not changed and are not run for this profile. This profile makes no claim for the three-voter topology.
