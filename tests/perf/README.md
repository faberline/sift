# Metric workload

`metric_workload.sh` starts a local `sift`, writes a fixed metric workload by
Prometheus Remote Write, runs a fixed query set, and prints one JSON line of
measured evidence.

Budgets are set by a person after reading measured numbers; this script has none.

## Run

```sh
WORKLOAD_REQUESTS=5 WORKLOAD_ITEMS=100 WORKLOAD_QUERIES=6 bash tests/perf/metric_workload.sh
```

Settings, all from the environment:

| Variable | Default | Meaning |
|---|---|---|
| `SIFT_BIN` | `<target>/debug/sift` | sift binary; built with cargo when missing |
| `WORKLOAD_REQUESTS` | 50 | remote write requests |
| `WORKLOAD_ITEMS` | 1000 | samples per request, 1 to 1000 |
| `WORKLOAD_QUERIES` | 20 | range queries, cycling over three fixed PromQL strings |
| `WORKLOAD_PROJECT` | perf | project written and queried |
| `WORKLOAD_OUT` | unset | optional file that also receives the JSON line |

Exit codes: 0 for a run with work; 1 for a failed write or query, a server
that does not start, or zero work; 2 for a bad setting or a host without Linux `/proc`.

Linux only: the script reads `/proc` for memory and CPU.

## JSON fields

One line on stdout, keys in this order:

- `samples_written`: requests times items per request.
- `queried_samples`: data points in the query responses.
- `samples_per_second`: samples written divided by the write phase time.
- `query_p50_ms`, `query_p95_ms`, `query_p99_ms`: query latency, nearest-rank method.
- `peak_rss_kib`: `VmHWM` of the sift process.
- `cpu_seconds`: user plus system CPU time of the sift process.
- `requests`, `items_per_request`, `queries`: the settings used.

All progress output goes to stderr.

## Regression check

`check_regression.py` compares one result with a stored baseline:

    python3 check_regression.py <result file> <baseline file> <threshold percent>

The baseline is `baseline/metric.json`. It holds the six measured keys:
`samples_per_second`, `query_p50_ms`, `query_p95_ms`, `query_p99_ms`,
`peak_rss_kib` and `cpu_seconds`.

- Throughput (`samples_per_second`) regresses when it is below
  `baseline * (1 - threshold / 100)`.
- Latency, memory and CPU keys regress when they are above
  `baseline * (1 + threshold / 100)`. A value at the limit passes.

Exit codes: 0 pass (prints `perf ok`), 1 regression (one line per key on
stderr), 2 bad input (missing key, unreadable file, bad threshold).

Unit tests: `cd tests/perf && python3 -B -m unittest test_check_regression -v`.

# Log workload

`log_workload.sh` starts a local `sift`, writes a fixed log workload as OTLP
logs (protobuf) to `/v1/logs`, runs three fixed log queries, and prints one
JSON line of measured evidence.

It takes the same settings as the metric workload (`SIFT_BIN`,
`WORKLOAD_REQUESTS`, `WORKLOAD_ITEMS`, `WORKLOAD_QUERIES`, `WORKLOAD_PROJECT`,
`WORKLOAD_OUT`, `WORKLOAD_BASE_MS`). `samples_written` counts log records
written, and `queried_samples` counts records returned by the queries. The
three fixed queries are log queries with no time range: all logs, logs with
`severity_number` 9, and the one record `wl-0-0`.

`check_regression.py` checks a log result against `baseline/log.json` with
the same six keys and the same rules as the metric baseline.

    bash tests/perf/log_workload.sh
    python3 -B tests/perf/check_regression.py <result file> tests/perf/baseline/log.json 60

## Trace workload

`trace_workload.sh` starts a local `sift`, writes a fixed trace workload as
OTLP protobuf to `/v1/traces` (one trace per request), reads the written
traces back with `GET /api/v1/traces/{trace_id}`, and prints one JSON line
of measured evidence.

Settings, all from the environment:

| Variable | Default | Meaning |
|---|---|---|
| `SIFT_BIN` | `<target>/debug/sift` | sift binary; built with cargo when missing |
| `WORKLOAD_REQUESTS` | 50 | trace write requests; one trace per request |
| `WORKLOAD_ITEMS` | 1000 | spans per trace, 1 to 1000 |
| `WORKLOAD_QUERIES` | 21 | trace reads, cycling over the written traces |
| `WORKLOAD_PROJECT` | perf | project written and read |
| `WORKLOAD_OUT` | unset | optional file that also receives the JSON line |

`samples_written` counts the spans written (requests times spans per trace).
`queried_samples` counts the spans read back in the trace responses. The
JSON keys and the exit codes are the same as in the metric workload.

`check_regression.py` checks the result against `baseline/trace.json`. That
baseline is provisional: it comes from a local run, not from the CI machine
class. Run the checker the same way as for the metric workload.
