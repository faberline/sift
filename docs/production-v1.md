# Sift 0.2.0 production contract

This release serves our applications on Standard GKE. Use three store voters
and three control voters. Keep membership and shard placement fixed.
Sift owns collection, storage, queries, error groups, and alerts.
Operators use the CLI, HTTP API, or read-only MCP tools.

## Inputs and identity

Keep the structured service log format. Accept JSON and text logs too.
Allow a configured multiline start pattern. Keep each record's original text,
source position, and Kubernetes identity. Persist pending deliveries before
advancing the source position. Keep rotation and restart positions.
Expose rejected records and missing source data.

Agents discover annotated Kubernetes metric targets. An agent scrapes only
Pods assigned to its own node. The default interval is 30 seconds.
Accept Prometheus text 0.0.4 and direct OTLP over JSON, Protobuf, or gRPC.
Normalize these inputs into the same metric model. Keep histogram buckets,
counts, sum, start time, end time, and temporality. Keep resource identity.
Test cumulative and delta values, counter resets, and resource collisions.

## Queries

Keep the V1 request and response behavior. Add V2 metric expressions and exact
log counts. API, CLI, MCP, and alerts use the same query service.
Use `promql-parser` 0.11.0 for syntax parsing. Sift executes the expression.
Keep the parser outside the pure domain layer.

Support selectors, label filters, range windows, grouped aggregates, `rate`,
`increase`, arithmetic, ratios, comparisons, and `histogram_quantile`.
Support identical label sets and explicit one-to-one label matching.
Reject many-to-many matching, subqueries, and other functions with an error.
Compare results with a fixed official Prometheus image.
Keep scan and memory limits. Report limit failures and incomplete data.
Never turn a sampled page into an exact count.

## Control data and operation

Add `control`, `error_reporting`, and `alerting` contexts in this crate.
Keep the architecture policy and zero exceptions.
Use the control Raft group for rules, saved queries, error status, alert state,
and pending notifications. Raft voters confirm each durable command together.
Only the current control leader evaluates rules and sends notifications.

Group errors by project, environment, service, exception type, and normalized
stack or message. Keep counts, first and last time, versions, and raw samples.
States are `open`, `resolved`, and `ignored`. Reopen a resolved group on a new
occurrence. Continue to count ignored groups.

Alert sources are metric conditions, log counts, new errors, and error recurrence.
Support hold time, no data, deduplication, mute, and recovery notifications.
A failed or incomplete query produces `unknown`. It cannot prove recovery.
Persist the notification before sending it. Retries use the same notification ID.
Use a generic Webhook. Read its settings from a Kubernetes Secret reference.
Allow network access only to configured destinations.

Expose `/api/v1/alert-rules`, `/api/v1/alerts`, `/api/v1/error-groups`, and
`/api/v1/saved-queries`. Require project admin rights for management writes.
Collectors have only the required input-read and telemetry-write rights.
MCP remains read-only. Keep GCP external probes for Sift availability.

## Recovery and capacity

Keep incremental GCS archives. Commit a complete recovery record every five
minutes. It must bind the data and the control state to one recovery point.
One lost node must lose zero acknowledged data. Whole-disk loss has a maximum
15-minute recovery point gap. If remote protection is more than 15 minutes
behind, reject new writes with a retryable error. Agents keep queued data.

Expose collection lag, pending delivery, query failures, notification failures,
backup lag, and disk capacity. Measure real item sizes before sizing PVCs and
GCS or approving test spend. A mock fixture is not a capacity estimate.

## Candidate acceptance

Run locked builds, Clippy, the full test suite, and strict architecture checks
at each phase. Run the required ignored tests with their real dependencies.
Keep prior tests. Cover source rotation, replay, multiline exceptions, resource
identity, project isolation, all metric transports, missing and stale points,
histogram quantiles, alert hold time, silence, no data, retry, restart, and
leader change. Compare all state after recovery.

The same immutable candidate image must pass both normal load tests:

| Gate | Exact load | Latency gate |
| --- | --- | --- |
| Short | 10,000 items/s for 30 minutes; 18,000,000 items | Logs and metrics p95 <= 2s; traces p95 <= 1s |
| Soak | 10,000 items/s for 24 hours; 864,000,000 items | Same query limits; Webhook p95 <= 60s after the condition holds |

Run queries and alerts while writing. Record each signal count and content
digest. Inject node failure, PVC replacement, long GCS outage, and whole-disk
restore separately. Bind every receipt to the source commit, candidate run,
root image digest, platform digest, configuration, time window, and evidence
hashes. A prior 1x1 receipt cannot satisfy this production contract.

Dual-write for seven days. Compare same-source logs, metric values, trace span
sets, and alert events. Send Sift alerts to a verification Webhook first.
Start with the service with the most logs and the service with the largest
peak. Enable production notifications one service at a time after acceptance.
Stop the switch on any data difference, restore failure, or capacity breach.
Keep the GCP rollback settings. Services above the validated 10,000 items/s
boundary remain on GCP until the next capacity tier passes.

## Local delivery and live acceptance

Run `./test.sh --candidate` from a clean checkout of the candidate commit.
The gate builds Sift with the lock file. It runs the full suite, all 24 required
environment tests, Clippy, and the strict architecture check. It also runs the
official Prometheus 3.5.0 oracle, Remote Write compliance, and Docker volume
replacement. External service versions are in `tests/environment.lock.json`.
These local checks do not prove GKE load or recovery.
The final check writes `target/sift-candidate-check.json` for the exact clean
source. The CI job retains this report. Test filters are refused in candidate mode.

Copy `scripts/production/config.example.json` to a private work directory.
Set the approved cost cap and current cost estimate. Include the cost of fault
drills and verification. Measure store bytes per item with application data.
The existing local segment policy keeps 30 days of hot data.
A five-minute archive commit does not remove all local segment data.
The preparation step reserves twice the measured 30-day store footprint
on each store replica at 10,000 items per second.
Measure stored bytes across logs, metrics, and traces. Size control data too.
Include these PVC sizes in the cost estimate. Verify disk use during load.

Save `gcloud container clusters describe` JSON to `cluster.json`.
Save at least 100 matching application log entries to `samples.json`.
Use the container names and namespaces in the configuration. System logs do
not qualify. Do not commit raw logs, credentials, or Secret contents.

```sh
python3 -B scripts/production/prepare.py \
  --candidate-manifest /private/tmp/sift-production/final-candidate-manifest.json \
  --config /private/tmp/sift-production/config.json \
  --cluster /private/tmp/sift-production/cluster.json \
  --samples /private/tmp/sift-production/samples.json \
  --output-dir /private/tmp/sift-production/prepared
```

The command writes `instance.json` and `preflight.json`. It does not deploy.
Before deployment, verify three ready nodes with separate placement for each
voter group. Configure GCS access, peer TLS, the Webhook Secret, and the GCP
external probe. Use the candidate digest in every Sift workload.

After deployment, read the live nodes and Pods with the runtime check.
It checks the permanent cluster ID, image digests, voter placement, readiness,
and Secret mounts. It reads metadata. It does not deploy or certify load.

```sh
python3 -B scripts/production/verify_runtime.py \
  --candidate-manifest /private/tmp/sift-production/final-candidate-manifest.json \
  --prepared-dir /private/tmp/sift-production/prepared \
  --kubeconfig /private/tmp/sift-production/kubeconfig \
  --output /private/tmp/sift-production/runtime-check.json
```

Expose log collector metrics with `sift collect --metrics-port 9318`.
The generated GKE log agent exposes this port. Its node-local metrics agent
scrapes it every 30 seconds. View rejected input in the quarantine file.
Source loss counters survive collector restart.
The log agent collects only the Sift instance namespace.
Use `sift collect --cri-root /var/log/pods --namespace NAME` for this scope.
The metrics agent discovers Pods in the Sift instance namespace.
Mark application Pods with `sift.axiom.dev/scrape=true`.
Set `prometheus.io/scrape: "true"`, the port, and the path in Pod annotations.
Use a separate namespace instance when project isolation requires it.
The agent queues metric points on disk before delivery.
Its log agent metrics endpoint reports pending logs and metric points.

For a whole-disk restore, save one `recovery/latest.json` file first.
Pass that file to every offline restore with `sift restore-complete --record-file`.
Use empty directories for the store and control data. The command verifies
both hashes and recovery positions. It keeps notification IDs and resets the
control Raft position for a fresh fixed membership group.

Run each load gate from a stable host with the prepared inputs.
The driver writes 8,000 logs, 1,000 metric points, and 1,000 spans per second.
It uses the measured application payloads for log bodies.
It reads back every item while writes continue. It checks resource identity,
raw log content, metric values, bucket counts, times, and complete span sets.
It observes a live alert rule throughout the run.
It keeps pending inputs on disk if the run fails.
It refuses partial responses, missing data, catch-up bursts, mixed images,
and query times above the limit. An interrupted run cannot pass.

The driver opens its own local port forward to the verified gateway Pod.
It checks the Pod UID first. It closes the forward after the run.
Use a private rotating token file with project admin access.
The Webhook reference must already exist in the configured Secret.
Run `load_30m` first. Use `load_24h` with the same candidate after it passes.

```sh
python3 -B scripts/production/run_load.py \
  --candidate-manifest /private/tmp/sift-production/final-candidate-manifest.json \
  --prepared-dir /private/tmp/sift-production/prepared \
  --samples /private/tmp/sift-production/samples.json \
  --kubeconfig /private/tmp/sift-production/kubeconfig \
  --token-file /private/tmp/sift-production/admin-token \
  --webhook-reference validation \
  --gate load_30m \
  --output-dir /private/tmp/sift-production/load-30m
```

The load output proves only its own gate. Measure Webhook delivery separately.
Keep the per-second input and read hashes in `verified.jsonl`.
The raw payload files are private. Do not commit them.
Fault drills and seven-day comparisons remain required.
Use complete source exports for each comparison window.
Each JSONL export starts with a `sift.production-observations/v1` header.
The header includes origin, candidate, cluster hash, time window, services,
and exact counts for logs, metrics, traces, alerts, and control records.
Set `complete: true`, `sampled: false`, and `truncated: false` only after all
source pages have been read. Each data row has `signal`, `identity`, and `content`.
Identity includes project, environment, and the source record key.
Content includes the full normalized source payload and resource identity.
Use the same normalization for both providers. Keep raw exports private.

`scripts/production/compare_exports.py` compares all rows with a disk index.
It detects missing records, extra records, changed contents, and duplicates.
Use `--mode recovery` for Sift before and after a fault.
Use `--mode dual-write` for GCP and Sift exports of the same source window.
One matching window does not prove seven days or export completeness.
The application-specific export adapters and continuous coverage must be
verified with the selected source services before cutover.
Put each measured gate record in `evidence/GATE.json` beside `acceptance.json`.
The acceptance document must contain the exact SHA-256 of each gate file.
Use `scripts/release/production_receipt.py` as the gate field contract.
The release receipt generator requires `--production-acceptance` for 0.2.0.
Missing tests, changed images, missing raw gate records, and exceeded limits
refuse release. Synthetic self-tests never qualify a production release.

## Reference contracts

- [PromQL parser 0.11.0](https://github.com/GreptimeTeam/promql-parser/releases/tag/v0.11.0)
- [Prometheus functions](https://prometheus.io/docs/prometheus/latest/querying/functions/)
- [OTLP metric model](https://opentelemetry.io/docs/specs/otel/metrics/data-model/)
