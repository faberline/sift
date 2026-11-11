# Prometheus comparison

This comparison runs Sift and the official Prometheus on the same metric workload and
checks the supported metric semantics: instant and range queries of
`sift_acceptance_total`, `sum(sift_acceptance_total)` and `count(sift_acceptance_total)`.
It also records write throughput, query latency, peak memory and CPU time of both sides.
It is evidence, not a gate: a difference is written down and a person reads it.

## Pin

- Prometheus version: `3.16.0`
- URL: `https://github.com/prometheus/prometheus/releases/download/v3.16.0/prometheus-3.16.0.linux-amd64.tar.gz`
- sha256: `68727009472a2c02e1ff7f062fd5b3e637285bffc8fc2d340c76fb449d6e0f34`

The script fails when the downloaded archive has another sha256.

## Workload

- `WORKLOAD_REQUESTS` (default 50), `WORKLOAD_ITEMS` (default 1000, at most 1000),
  `WORKLOAD_QUERIES` (default 20).
- Both servers receive the same payload bytes with the same timestamps, built from
  one start time.
- The three PromQL strings run in turn. Each range query uses the same window:
  start at the first sample time, end two seconds after the last one, step 1 s.

## Run

- Local: `bash tests/perf/prometheus_comparison.sh`. It needs Linux, `curl`, `python3`,
  `tar`, `sha256sum` and a built `sift`.
- On the runner: the workflow `sift-prometheus-comparison` (manual, `ubuntu-24.04`).
  Its output is in the step summary and in the uploaded artifact.

## Result fields

- `prometheus_version`, `prometheus_sha256`: the pin the run used.
- `workload`: requests, items per request and queries.
- `sift`: the JSON line of `tests/perf/metric_workload.sh` for Sift.
- `prometheus`: samples written, queried samples, samples per second, query p50 and p95
  in milliseconds, peak RSS in KiB and CPU seconds.
- `semantic`: the number of queries, how many match sample by sample, and up to five
  differences.

## Rules for a difference

A difference in `semantic.differences` is read by a person. It is either written in this
document as an intentional delta with its reason, or linked as one bounded issue with
`tracked_skip(#<issue>)`. A difference is never hidden by changing the comparison.

## Recorded results

No run is recorded yet. After the first run of the workflow a person adds the table: date,
Sift commit, `semantic`, and both sides' numbers, copied from the workflow's step summary.

| date | sift commit | matching / queries | sift samples/s | prometheus samples/s | sift p95 ms | prometheus p95 ms | sift peak RSS KiB | prometheus peak RSS KiB |
|---|---|---|---|---|---|---|---|---|

## Cleanup

The script removes its temporary directory and stops Prometheus when it ends. The runner is
discarded after the job. Nothing paid is used and nothing is left running.

## Out of scope

Old profile, audit, GenAI and public replay comparisons. GKE (#16).
