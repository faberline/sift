# Single-node profile contract (single-node-v1)

## Scope

The profile is one `all`-role Pod on one PVC on Standard GKE, installed by the operator with `topology: singleNode` (issues #220 to #222 of #214). It is a small-volume profile for one team. The acceptance run waits for #218 and #220 to #231.

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

## Acceptance gates

Every gate is **not run**. No live GKE evidence exists yet.

1. **G1** One documented install on Standard GKE with a single node, no GCS and no FQDN policy, that collects the container logs and the Prometheus metrics of one namespace and answers `sift_query` and PromQL for them.
2. **G2** Retention set to 30 days is enforced.
3. **G3** Logs of a Pod that is deleted within 10 s of exit are queryable.
4. **G4** The functional and PromQL-oracle gates that already run in the CI workflow `.github/workflows/ci.yml` with the default `serve --role all` (the default of `--role` in `src/bin/sift/serve_args.rs` is `all`):
   - `test: compat`: its step `prometheus compliance` runs `tests/prometheus_compliance.sh`, which starts `sift serve` with the default role and compares PromQL answers with the oracle.
   - `test: perf (metric workload)`: its step `metric workload` runs `tests/perf/metric_workload.sh`, which starts `sift serve` with the default role and runs the metric workload.

   These gates run on a CI runner, not on GKE.

## Senders

**Basis.** Read from the code of this repository, not measured, not a GKE claim. No live GKE evidence exists yet.

**Agents and nodes.** One log agent and one metric agent run for each node. The log agent is the DaemonSet of `agent_daemon_set_plan` in `src/operations/infrastructure/k8s/agent_workloads.rs`; it reads `/var/log/pods` of its own node. The metric agent is a second DaemonSet, named `<name>-agent-metrics` (`src/operations/infrastructure/k8s/metrics_agent.rs`). The log agent requests 25m CPU and 64Mi memory and is limited to 500m CPU and 256Mi memory (`src/operations/infrastructure/k8s/agent_workloads.rs`). Its data path is `/var/lib/sift/agent` (same file). With N nodes there are 2N agent Pods, and each one reads or scrapes only its own node.

**Cost for each Pod.** The log agent keeps one checkpoint entry for each log file, in one JSON file, `/var/lib/sift/agent/checkpoint.json` (`src/operations/infrastructure/k8s/agent_workloads.rs`). The entry is keyed by device, inode and birth time, and has the fields `offset`, `line`, `observed_len`, `relative_path`, `workload`, `retired`, `loss_reported`, `generation`, `anchor` and `birth_nanos` (`src/collector/infrastructure/cri_checkpoint.rs`). The code gives no byte size for an entry: bytes for each entry are not measured, and no constant for them exists in the code. Entries of removed files stay in the map with `retired: true`; the code that sets `file.retired = true` (`src/collector/infrastructure/cri_source/state.rs`) does not delete them. A node with many short-lived Pods therefore grows the checkpoint file; no rate is claimed. CRI discovery reads the directory tree under the root to a depth of 3 and skips symlinks (`src/collector/infrastructure/cri_discovery.rs`). The metric agent keeps no state for each Pod except the queue below.

**Store down: logs.** The agent runs with `--follow` (`src/operations/infrastructure/k8s/agent_workloads.rs`), so its delivery retry mode is `DeliveryRetryMode::UntilCancelled` (`src/collector/application/runtime.rs`): it retries a failed delivery without limit and does not read past the batch. It holds one pending batch on disk, a JSON file written with fsync before each send, of at most 1000 events and 64 MiB (`src/collector/infrastructure/durable_delivery.rs`). A bigger batch is refused as a permanent failure. After that one batch the agent stops reading. It drops nothing itself: the CRI source keeps the file offset in the checkpoint, and the log lines stay in the node log files (`src/collector/infrastructure/cri_source/state.rs`). What the node does with its log files while the agent waits (rotation) is the kubelet setting; it is not in this code, and no number is given for it.

**Store down: metrics.** The metric agent scrapes the Pods of its node one after another. One scrape of one Pod is split into batches of 1000 samples (`src/collector/infrastructure/metric_scraper.rs`), and the batches are saved to `pending-metrics.json` before the first send, at most 64 MiB (`src/collector/infrastructure/metric_queue.rs`). While the store is down, it stays on the current Pod and retries every `min(interval, 30 s)` (`src/collector/infrastructure/metric_scraper.rs`); the interval defaults to 30 s (`src/bin/sift/scrape.rs`). It scrapes no other Pod in that time. It drops nothing, but the Pods that wait lose their scrape intervals: no sample is made for the time of the wait. One scrape response is at most 8 MiB. The scrape port defaults to 9090 and the path to `/metrics` (`src/collector/infrastructure/metric_scraper.rs`).

**Retry policy of the sender client.**

- One-shot mode retries 3 times by default (`DEFAULT_MAX_RETRIES` in `src/collector/domain/config.rs`; the config allows at most 20), and each wait is capped at 5 s (`src/collector/application/runtime.rs`).
- It retries HTTP 429, every 5xx status and a transport error. All other statuses and a partial success are permanent (`src/collector/infrastructure/client.rs`).
- The request timeout is 10 s by default (`--request-timeout-secs`, `src/bin/sift/collect.rs`).
- The batch size is 100 by default and at most 1000; the line limit is 512 KiB by default and at most 1 MiB (`src/collector/domain/config.rs`).

**Hundreds of Pods in several namespaces: is anything needed beyond the namespace lists?** **Yes**, with this evidence:

- The render passes one namespace, the namespace of the `Sift` object (`cx.ns()`), to both agents (`src/operations/infrastructure/k8s/agent_workloads.rs` and `src/operations/infrastructure/k8s/metrics_agent.rs`). Both agents already take a comma-separated list (`value_delimiter = ','` in `src/bin/sift/collect.rs` and `src/bin/sift/scrape.rs`), but the render does not pass a list on `main`; #224 and #227 add it.
- The metric agent scrapes the Pods of its node one after another (`src/collector/infrastructure/metric_scraper.rs`), so one round takes the sum of the Pod scrape times. For many Pods on one node that sum can exceed the 30 s interval. This follows from the loop; no number is given.
- A Pod needs both the annotation `prometheus.io/scrape` = `true` and the label `sift.axiom.dev/scrape` = `true` (`src/collector/infrastructure/metric_scraper.rs`), so a namespace list alone does not select Pods.

This issue makes no change to the code.

## Relation to production-v1

[production-v1.md](production-v1.md) and its 13-gate acceptance are not changed and are not run for this profile. This profile makes no claim for the three-voter topology.
