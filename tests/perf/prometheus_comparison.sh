#!/usr/bin/env bash
set -euo pipefail

# Compares Sift with the pinned official Prometheus on the metric workload of
# tests/perf/metric_workload.sh and prints one JSON line of evidence.
# It is evidence, not a gate: a difference in query results is reported in
# "semantic.differences" and still exits 0. It sets no speed, memory or latency
# threshold.
#
# Settings (environment, with defaults):
#   SIFT_BIN                 sift binary (default: <target>/debug/sift, built when missing)
#   WORKLOAD_REQUESTS        remote write requests (default 50)
#   WORKLOAD_ITEMS           samples per request, 1..1000 (default 1000)
#   WORKLOAD_QUERIES         range queries (default 20)
#   WORKLOAD_PROJECT         project name (default perf)
#   COMPARISON_OUT           optional file that also receives the JSON line
#   PROMETHEUS_URL           archive URL (default: the pinned release URL)
#   PROMETHEUS_SHA256_OVERRIDE_FOR_TESTS
#                            test-only digest, honoured ONLY when PROMETHEUS_URL
#                            starts with file://; ignored for every other URL.
#                            The tests need it because their stub archive is not
#                            the pinned one.

PROMETHEUS_VERSION="3.16.0"
PROMETHEUS_SHA256="68727009472a2c02e1ff7f062fd5b3e637285bffc8fc2d340c76fb449d6e0f34"
PROMETHEUS_URL_DEFAULT="https://github.com/prometheus/prometheus/releases/download/v${PROMETHEUS_VERSION}/prometheus-${PROMETHEUS_VERSION}.linux-amd64.tar.gz"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="${SIFT_REPO_ROOT:-$(git -c core.fsmonitor=false rev-parse --show-toplevel)}"

log() {
  echo "prometheus_comparison: $*" >&2
}

for command in curl python3 tar sha256sum; do
  command -v "$command" >/dev/null 2>&1 || {
    log "required command not found: $command"
    exit 1
  }
done

if [[ ! -r /proc/self/status ]]; then
  echo "prometheus_comparison.sh needs Linux /proc" >&2
  exit 2
fi

requests="${WORKLOAD_REQUESTS:-50}"
items="${WORKLOAD_ITEMS:-1000}"
queries="${WORKLOAD_QUERIES:-20}"
project="${WORKLOAD_PROJECT:-perf}"
out_file="${COMPARISON_OUT:-}"
prometheus_url="${PROMETHEUS_URL:-$PROMETHEUS_URL_DEFAULT}"

check_number() {
  local name="$1" value="$2"
  [[ "$value" =~ ^[0-9]+$ ]] || {
    echo "$name must be a whole number: $value" >&2
    exit 2
  }
}
check_number WORKLOAD_REQUESTS "$requests"
check_number WORKLOAD_ITEMS "$items"
check_number WORKLOAD_QUERIES "$queries"

if (( items > 1000 )); then
  echo "WORKLOAD_ITEMS must be at most 1000: $items" >&2
  exit 2
fi

if (( requests == 0 || items == 0 || queries == 0 )); then
  echo "workload size must be above zero" >&2
  exit 1
fi

sift_bin="${SIFT_BIN:-${CARGO_TARGET_DIR:-$repo_root/target}/debug/sift}"
if [[ ! -x "$sift_bin" ]]; then
  cargo build --locked -p sift --bin sift >&2
fi
[[ -x "$sift_bin" ]] || {
  echo "sift binary not found: $sift_bin" >&2
  exit 1
}

pick_port() {
  python3 -I -c 'import socket;s=socket.socket();s.bind(("127.0.0.1",0));print(s.getsockname()[1])'
}

work_root="$(mktemp -d)"
prom_pid=""
cleanup() {
  if [[ -n "$prom_pid" ]]; then
    kill "$prom_pid" 2>/dev/null || true
    wait "$prom_pid" 2>/dev/null || true
  fi
  if [[ -d "$work_root" ]]; then
    find "$work_root" -depth -delete
  fi
}
trap cleanup EXIT INT TERM

# Download the archive and check its digest before anything is extracted.
expected_sha="$PROMETHEUS_SHA256"
if [[ "$prometheus_url" == file://* && -n "${PROMETHEUS_SHA256_OVERRIDE_FOR_TESTS:-}" ]]; then
  expected_sha="$PROMETHEUS_SHA256_OVERRIDE_FOR_TESTS"
fi
archive="$work_root/prometheus.tar.gz"
log "downloading prometheus $PROMETHEUS_VERSION"
if ! curl -fsSL --connect-timeout 10 --max-time 900 -o "$archive" "$prometheus_url"; then
  log "download failed: $prometheus_url"
  exit 1
fi
actual_sha="$(sha256sum "$archive" | cut -d' ' -f1)"
if [[ "$actual_sha" != "$expected_sha" ]]; then
  echo "prometheus archive digest mismatch: expected $expected_sha got $actual_sha" >&2
  exit 1
fi
mkdir -p "$work_root/prom"
tar -xzf "$archive" -C "$work_root/prom"
prom_bin="$work_root/prom/prometheus-${PROMETHEUS_VERSION}.linux-amd64/prometheus"
if [[ ! -x "$prom_bin" ]]; then
  echo "prometheus binary not executable: $prom_bin" >&2
  exit 1
fi

mkdir -p "$work_root/payloads"
base_ms=$(( $(date +%s) * 1000 ))

# Sift side: metric_workload.sh starts Sift, runs the workload and stops Sift
# before it returns, so the two servers never run together.
SIFT_BIN="$sift_bin" WORKLOAD_BASE_MS="$base_ms" \
  WORKLOAD_KEEP_RESPONSES="$work_root/sift_q" WORKLOAD_OUT="$work_root/sift.json" \
  WORKLOAD_REQUESTS="$requests" WORKLOAD_ITEMS="$items" \
  WORKLOAD_QUERIES="$queries" WORKLOAD_PROJECT="$project" \
  bash "$script_dir/metric_workload.sh" >&2

# Prometheus side.
prom_port="$(pick_port)"
mkdir -p "$work_root/prom-data"
cat > "$work_root/prometheus.yml" <<'YAML'
global:
  scrape_interval: 1m
YAML
"$prom_bin" --config.file="$work_root/prometheus.yml" \
  --storage.tsdb.path="$work_root/prom-data" \
  --web.listen-address="127.0.0.1:${prom_port}" \
  --web.enable-remote-write-receiver --log.level=warn \
  >"$work_root/prometheus.log" 2>&1 &
prom_pid=$!

ready=0
for _ in $(seq 1 100); do
  if ! kill -0 "$prom_pid" 2>/dev/null; then
    break
  fi
  if curl -fsS "http://127.0.0.1:${prom_port}/-/ready" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 0.1
done
if (( ready == 0 )); then
  echo "prometheus did not become ready; head of prometheus.log:" >&2
  head -n 40 "$work_root/prometheus.log" >&2 || true
  exit 1
fi

# Build every payload before the timed window, with the same bytes as Sift got.
for (( i = 0; i < requests; i++ )); do
  "$sift_bin" acceptance-payload --kind prometheus-remote-write-v1 \
    --items "$items" --project "$project" --event-prefix "wl-$i" \
    --timestamp-unix-nano "$(( (base_ms + i * items) * 1000000 ))" \
    > "$work_root/payloads/$i.snappy"
done

# Write phase: only the curl calls sit between t0 and t1.
t0="$(python3 -I -c 'import time;print(time.monotonic())')"
for (( i = 0; i < requests; i++ )); do
  status="$(curl -sS --output /dev/null --write-out '%{http_code}' \
    -X POST "http://127.0.0.1:${prom_port}/api/v1/write" \
    -H 'content-type: application/x-protobuf' \
    -H 'content-encoding: snappy' \
    -H 'x-prometheus-remote-write-version: 0.1.0' \
    --data-binary "@$work_root/payloads/$i.snappy" || echo 000)"
  if [[ "$status" != "204" ]]; then
    echo "prometheus remote write request $i answered $status" >&2
    exit 1
  fi
done
t1="$(python3 -I -c 'import time;print(time.monotonic())')"

# Query phase: the same window and the same PromQL as metric_workload.sh.
start=$(( base_ms / 1000 ))
end=$(( (base_ms + requests * items) / 1000 + 2 ))
query_texts=(
  'sift_acceptance_total'
  'sum(sift_acceptance_total)'
  'count(sift_acceptance_total)'
)
: > "$work_root/prom_times.txt"
for (( k = 0; k < queries; k++ )); do
  query="${query_texts[$(( k % 3 ))]}"
  timing="$(curl -sS -G "http://127.0.0.1:${prom_port}/api/v1/query_range" \
    --data-urlencode "query=$query" \
    --data-urlencode "start=$start" \
    --data-urlencode "end=$end" \
    --data-urlencode "step=1" \
    --output "$work_root/prom_q_$k.json" \
    --write-out '%{http_code} %{time_total}')"
  code="${timing%% *}"
  seconds="${timing#* }"
  if [[ "$code" != "200" ]]; then
    echo "prometheus query $k answered $code" >&2
    exit 1
  fi
  echo "$seconds" >> "$work_root/prom_times.txt"
done

# Read Prometheus numbers from /proc while it still runs, compare the query
# results sample by sample, and build the JSON line.
read -r -d '' compare_py <<'PY' || true
import json, math, os, sys

work, pid, requests, items, queries, t0, t1, out_path, version, sha = sys.argv[1:11]
requests, items, queries = int(requests), int(items), int(queries)
t0, t1 = float(t0), float(t1)
texts = ["sift_acceptance_total", "sum(sift_acceptance_total)", "count(sift_acceptance_total)"]

def load(path):
    with open(path, "rb") as handle:
        return json.load(handle)

def normalise(body):
    out = {}
    for series in body.get("data", {}).get("result", []):
        key = tuple(sorted(series.get("metric", {}).items()))
        out.setdefault(key, []).extend(
            (round(float(ts), 3), float(v)) for ts, v in series.get("values", [])
        )
    for key in out:
        out[key].sort()
    return out

def pairs(norm):
    return sum(len(values) for values in norm.values())

def first_difference(a, b):
    if set(a) != set(b):
        return "series set differs"
    if pairs(a) != pairs(b):
        return f"sample count differs (sift={pairs(a)} prometheus={pairs(b)})"
    for key in a:
        for (ts_a, v_a), (ts_b, v_b) in zip(a[key], b[key]):
            if ts_a != ts_b or not math.isclose(v_a, v_b, rel_tol=1e-9, abs_tol=1e-12):
                return f"value differs at {ts_a} (sift={v_a} prometheus={v_b})"
    return None

sift = load(os.path.join(work, "sift.json"))

prom_queried = 0
matching = 0
differences = []
for k in range(queries):
    sift_norm = normalise(load(os.path.join(work, "sift_q", f"q_{k}.json")))
    prom_norm = normalise(load(os.path.join(work, f"prom_q_{k}.json")))
    prom_queried += pairs(prom_norm)
    reason = first_difference(sift_norm, prom_norm)
    if reason is None:
        matching += 1
    elif len(differences) < 5:
        differences.append(f"query {k} {texts[k % 3]}: {reason}")

if prom_queried == 0:
    print("workload did no work: prometheus queried_samples=0", file=sys.stderr)
    sys.exit(1)

times_ms = sorted(float(x) * 1000.0 for x in open(os.path.join(work, "prom_times.txt")).read().split())

def nearest_rank(p):
    n = len(times_ms)
    return times_ms[max(math.ceil(p * n) - 1, 0)]

peak_rss_kib = 0
with open(f"/proc/{pid}/status") as handle:
    for line in handle:
        if line.startswith("VmHWM:"):
            peak_rss_kib = int(line.split()[1])

with open(f"/proc/{pid}/stat") as handle:
    stat = handle.read()
fields = stat.rsplit(")", 1)[1].split()
utime, stime = int(fields[11]), int(fields[12])
cpu_seconds = (utime + stime) / os.sysconf("SC_CLK_TCK")

samples = requests * items
elapsed = max(t1 - t0, 1e-6)

result = {
    "prometheus_version": version,
    "prometheus_sha256": sha,
    "workload": {"requests": requests, "items_per_request": items, "queries": queries},
    "sift": sift,
    "prometheus": {
        "samples_written": samples,
        "queried_samples": prom_queried,
        "samples_per_second": round(samples / elapsed, 3),
        "query_p50_ms": round(nearest_rank(0.50), 3),
        "query_p95_ms": round(nearest_rank(0.95), 3),
        "peak_rss_kib": peak_rss_kib,
        "cpu_seconds": round(cpu_seconds, 3),
    },
    "semantic": {"queries": queries, "matching": matching, "differences": differences},
}
line = json.dumps(result)
if out_path:
    with open(out_path, "w") as handle:
        handle.write(line + "\n")
print(line)
PY

if ! python3 -I -c "$compare_py" "$work_root" "$prom_pid" "$requests" "$items" \
  "$queries" "$t0" "$t1" "$out_file" "$PROMETHEUS_VERSION" "$PROMETHEUS_SHA256"; then
  exit 1
fi
