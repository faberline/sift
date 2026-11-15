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

CI passes two thresholds: `60` for throughput and `130` for the other keys.

- Throughput must not fall below 40 percent of the baseline.
- Query latency, peak RSS and CPU must not rise above 230 percent of the
  baseline. The limit is set so that the slowest measured run (p99 954.58 ms,
  CPU 44.78 s) passes with about 15 percent margin. A regression that is
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
| Throughput (samples per second) | 4874.039 |
| Query p50 (ms) | 6.751 |
| Query p95 (ms) | 12.382 |
| Query p99 (ms) | 86.856 |
| Peak RSS (KiB) | 501616 |
| CPU (seconds) | 15.54 |

These numbers come from the `perf-log` artifact of the CI job `test-perf` on
the `ubuntu-24.04` runner (run 43687463792). A person replaces them when a
later CI run shows the numbers are off.

The threshold is 60 percent, the same as in the metric section above
("Threshold"). The baseline is `tests/perf/baseline/log.json`.

## Not covered

- The paid load run of 10,000 items per second on GKE is not part of this
  gate. It is tracked by issue #83.
- The trace workload is tracked by issue #131.
