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
- `query_p50_ms`, `query_p95_ms`: query latency, nearest-rank method.
- `peak_rss_kib`: `VmHWM` of the sift process.
- `cpu_seconds`: user plus system CPU time of the sift process.
- `requests`, `items_per_request`, `queries`: the settings used.

All progress output goes to stderr.
