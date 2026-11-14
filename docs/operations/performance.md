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
| Throughput (samples per second) | 16232.955 |
| Query p50 (ms) | 71.919 |
| Query p95 (ms) | 113.95 |
| Query p99 (ms) | 252.746 |
| Peak RSS (KiB) | 276212 |
| CPU (seconds) | 5.68 |

The baseline came from one run in the cloud session, with the CI settings
above. The first CI artifact `perf-metric` is the first result from
the CI machine class. A person reads it and then updates the baseline.

## Machine class

GitHub-hosted runner `ubuntu-24.04`, job `test-perf`.

## Threshold

The threshold is 60 percent.

- Throughput must not fall below 40 percent of the baseline.
- Query latency, peak RSS and CPU must not rise above 160 percent of the
  baseline.

`check_regression.py` compares each key with the baseline. A value exactly at
the limit passes. Exit code 0 means pass, 1 means a regression, and 2 means
bad input. The gate is loose on purpose: it catches large regressions. It
does not measure small changes.

## How to update the baseline

1. Open the CI run of `test-perf` and download the artifact `perf-metric`.
2. Copy its six keys into `tests/perf/baseline/metric.json`.
3. Read the numbers before you change the threshold. Only a person changes
   the threshold in `ci.yml`.

## Not covered

- The paid load run of 10,000 items per second on GKE is not part of this
  gate. It is tracked by issue #83.
- Log and trace workloads are not part of this gate.
