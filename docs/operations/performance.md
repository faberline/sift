# Performance: metric workload

This page holds the measured numbers of the metric workload and the rule of
the CI gate that checks them. The gate is job `test-perf` in
`.github/workflows/ci.yml`. The checker is `tests/perf/check_regression.py`.

## What this measures

The workload is `tests/perf/metric_workload.sh`. It starts sift, writes
metric samples over HTTP, then runs three PromQL queries and reports one JSON
line.

CI settings: 50 requests of 1000 samples each, and 21 queries (each of the
three PromQL queries runs 7 times).

## Measured numbers

These are the values in `tests/perf/baseline/metric.json`.

| Metric | Value |
|---|---|
| Throughput (samples per second) | 4534.798 |
| Query p50 (ms) | 258.684 |
| Query p95 (ms) | 270.601 |
| Query p99 (ms) | 490.315 |
| Peak RSS (KiB) | 290336 |
| CPU (seconds) | 22.825 |

The numbers are medians of 18 CI runs of the CI job `test-perf` on the
`ubuntu-24.04` runner, made on 2026-11-14 and 2026-11-15, with the CI settings
above. The runner is either a fast or a slow machine. A slow machine is about
1.8 times slower, and one job keeps one machine. A person reads later CI
artifacts and updates the baseline when needed.

## Machine class

GitHub-hosted runner `ubuntu-24.04`, job `test-perf`.

## Threshold

CI passes two thresholds: `60` for throughput and `160` for the other keys.

- Throughput must not fall below 40 percent of the baseline.
- Query latency, peak RSS and CPU must not rise above 260 percent of the
  baseline. The limit is set so that the slowest of 36 measured runs (p99
  995.746 ms, CPU 44.78 s) passes with at least 28 percent margin. A regression that is
  smaller than the runner spread is not caught.

`check_regression.py` compares each key with the baseline. A value exactly at
the limit passes. Exit code 0 means pass, 1 means a regression, and 2 means
bad input. The gate is loose on purpose: it catches large regressions. It
does not measure small changes.

## How to update the baseline

1. Open the CI run of `test-perf` and download the artifact `perf-metric`.
2. Copy its six keys into `tests/perf/baseline/metric.json`.
3. Read the numbers before you change the threshold. Only a person changes
   the threshold in `ci.yml`.

## Log workload

The workload is `tests/perf/log_workload.sh`. It starts sift, writes OTLP logs
(protobuf) to `/v1/logs`, then runs three fixed log queries and reports one
JSON line.

Settings: 50 requests of 1000 records each, and 21 queries (each of the three
queries runs 7 times).

| Metric | Value |
|---|---|
| Throughput (samples per second) | 4358.413 |
| Query p50 (ms) | 7.05 |
| Query p95 (ms) | 16.675 |
| Query p99 (ms) | 87.077 |
| Peak RSS (KiB) | 503122 |
| CPU (seconds) | 16.975 |

These numbers are the medians of 18 CI runs of 2026-11-14 to 2026-11-16 on the
`ubuntu-24.04` runner (fast and slow machines mix). A person replaces them when
a later CI run shows the numbers are off.

CI passes the thresholds 60 for throughput and 150 for the other keys: latency,
peak RSS and CPU must not rise above 250 percent of the baseline. The key
`query_p99_ms` of this workload is not compared (`--ignore`): it varies from 78
to 272 ms between runs, 3.1 times the median, so no limit that catches a real
regression passes every run. The limits for p50, p95 and CPU pass the slowest
of 18 measured runs with at least 50 percent margin. The baseline is
`tests/perf/baseline/log.json`.

## Trace workload

The workload is `tests/perf/trace_workload.sh`. It starts sift, writes OTLP
traces (protobuf) to `/v1/traces`, then reads the traces back with
`GET /api/v1/traces/{trace_id}` and reports one JSON line.

Settings: 50 requests, one trace of 1000 spans per request, and 21 trace reads.

| Metric | Value |
|---|---|
| Throughput (samples per second) | 4029.727 |
| Query p50 (ms) | 676.363 |
| Query p95 (ms) | 680.546 |
| Query p99 (ms) | 683.482 |
| Peak RSS (KiB) | 466192 |
| CPU (seconds) | 32.65 |

These numbers come from the `perf-trace` artifact of the CI job `test-perf` on
the `ubuntu-24.04` runner. A person replaces them when a later CI run shows the
numbers are off.

CI runs the trace workload 5 times in one job and compares the median of each key with the baseline. The threshold is `60` for every key: throughput must not fall below 40 percent of the baseline, and latency, peak RSS and CPU must not rise above 160 percent of the baseline. The median ignores one or two slow runs. It does not help when the whole job runs on a slow machine, because the 5 runs share one machine. If the median fails, a person reads the per-run numbers in the `perf-trace` artifact (`perf-trace-1.json` to `perf-trace-5.json`) and decides; a pull request changes neither the threshold nor the baseline. The baseline is `tests/perf/baseline/trace.json`.

## Not covered

- The paid load run of 10,000 items per second on GKE is not part of this
  gate. It is tracked by issue #83.
