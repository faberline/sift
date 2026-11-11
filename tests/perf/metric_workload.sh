#!/usr/bin/env bash
set -euo pipefail

# Measures Sift on a fixed metric workload and prints one JSON line of evidence.
# It sets no budget: a person sets budgets after reading the measured numbers.
#
# Settings (environment, with defaults):
#   SIFT_BIN           sift binary (default: <target>/debug/sift, built when missing)
#   WORKLOAD_REQUESTS  remote write requests (default 50)
#   WORKLOAD_ITEMS     samples per request, 1..1000 (default 1000)
#   WORKLOAD_QUERIES   range queries (default 20)
#   WORKLOAD_PROJECT   project name (default perf)
#   WORKLOAD_OUT       optional file that also receives the JSON line

repo_root="${SIFT_REPO_ROOT:-$(git -c core.fsmonitor=false rev-parse --show-toplevel)}"

log() {
  echo "metric_workload: $*" >&2
}

for command in curl python3; do
  command -v "$command" >/dev/null 2>&1 || {
    log "required command not found: $command"
    exit 1
  }
done

if [[ ! -r /proc/self/status ]]; then
  echo "metric_workload.sh needs Linux /proc" >&2
  exit 2
fi

requests="${WORKLOAD_REQUESTS:-50}"
items="${WORKLOAD_ITEMS:-1000}"
queries="${WORKLOAD_QUERIES:-20}"
project="${WORKLOAD_PROJECT:-perf}"
out_file="${WORKLOAD_OUT:-}"

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
sift_pid=""
cleanup() {
  if [[ -n "$sift_pid" ]]; then
    kill "$sift_pid" 2>/dev/null || true
    wait "$sift_pid" 2>/dev/null || true
  fi
  if [[ -d "$work_root" ]]; then
    find "$work_root" -depth -delete
  fi
}
trap cleanup EXIT INT TERM

http_port="$(pick_port)"
grpc_port="$(pick_port)"
mkdir -p "$work_root/data" "$work_root/payloads"

log "starting sift on 127.0.0.1:$http_port"
SIFT_AUTH=off "$sift_bin" serve --host 127.0.0.1 --port "$http_port" \
  --grpc-port "$grpc_port" --data-dir "$work_root/data" \
  >"$work_root/sift.log" 2>&1 &
sift_pid=$!

ready=0
for _ in $(seq 1 100); do
  if ! kill -0 "$sift_pid" 2>/dev/null; then
    break
  fi
  if curl -fsS "http://127.0.0.1:${http_port}/readyz" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 0.1
done
if (( ready == 0 )); then
  echo "sift did not become ready; head of sift.log:" >&2
  head -n 40 "$work_root/sift.log" >&2 || true
  exit 1
fi

# Build every payload before the timed window.
base_ms=$(( $(date +%s) * 1000 ))
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
    -X POST "http://127.0.0.1:${http_port}/prometheus/api/v1/write" \
    -H 'content-type: application/x-protobuf' \
    -H 'content-encoding: snappy' \
    -H 'x-prometheus-remote-write-version: 0.1.0' \
    --data-binary "@$work_root/payloads/$i.snappy" || echo 000)"
  if [[ "$status" != "204" ]]; then
    echo "remote write request $i answered $status" >&2
    exit 1
  fi
done
t1="$(python3 -I -c 'import time;print(time.monotonic())')"

# Query phase.
start=$(( base_ms / 1000 ))
end=$(( (base_ms + requests * items) / 1000 + 2 ))
query_texts=(
  'sift_acceptance_total'
  'sum(sift_acceptance_total)'
  'count(sift_acceptance_total)'
)
: > "$work_root/times.txt"
for (( k = 0; k < queries; k++ )); do
  query="${query_texts[$(( k % 3 ))]}"
  timing="$(curl -sS -G "http://127.0.0.1:${http_port}/prometheus/api/v1/query_range" \
    --data-urlencode "project=$project" \
    --data-urlencode "query=$query" \
    --data-urlencode "start=$start" \
    --data-urlencode "end=$end" \
    --data-urlencode "step=1" \
    --output "$work_root/q_$k.json" \
    --write-out '%{http_code} %{time_total}')"
  code="${timing%% *}"
  seconds="${timing#* }"
  if [[ "$code" != "200" ]]; then
    echo "query $k answered $code" >&2
    exit 1
  fi
  echo "$seconds" >> "$work_root/times.txt"
done

# Numbers are read from /proc while sift still runs; the JSON line is built here.
read -r -d '' stats_py <<'PY' || true
import json, math, os, sys

work, pid, requests, items, queries, t0, t1, out_path = sys.argv[1:9]
requests, items, queries = int(requests), int(items), int(queries)
t0, t1 = float(t0), float(t1)

times_ms = sorted(float(x) * 1000.0 for x in open(os.path.join(work, "times.txt")).read().split())

queried = 0
for k in range(queries):
    with open(os.path.join(work, f"q_{k}.json"), "rb") as handle:
        body = json.load(handle)
    for series in body.get("data", {}).get("result", []):
        queried += len(series.get("values", []))

samples = requests * items
elapsed = max(t1 - t0, 1e-6)

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

if queried == 0:
    print("workload did no work: queried_samples=0", file=sys.stderr)
    sys.exit(1)

result = {
    "samples_written": samples,
    "queried_samples": queried,
    "samples_per_second": round(samples / elapsed, 3),
    "query_p50_ms": round(nearest_rank(0.50), 3),
    "query_p95_ms": round(nearest_rank(0.95), 3),
    "peak_rss_kib": peak_rss_kib,
    "cpu_seconds": round(cpu_seconds, 3),
    "requests": requests,
    "items_per_request": items,
    "queries": queries,
}
line = json.dumps(result)
with open(out_path, "w") as handle:
    handle.write(line + "\n")
print(line)
PY

result_file="$work_root/result.json"
if ! python3 -I -c "$stats_py" "$work_root" "$sift_pid" "$requests" "$items" \
  "$queries" "$t0" "$t1" "$result_file"; then
  exit 1
fi

if [[ -n "$out_file" ]]; then
  cp "$result_file" "$out_file"
fi
