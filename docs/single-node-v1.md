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

## Relation to production-v1

[production-v1.md](production-v1.md) and its 13-gate acceptance are not changed and are not run for this profile. This profile makes no claim for the three-voter topology.
