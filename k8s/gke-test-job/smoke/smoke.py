"""Generate one structured log and verify its recovery after a pod restart."""

import gzip
import http.client
import json
import os
import ssl
import struct
import sys
import time
import urllib.error
import urllib.request
import uuid
from datetime import datetime, timezone
from pathlib import Path


POD = "sift-smoke-0"
CLAIM = "data-sift-smoke-0"
SOURCE = Path("event.jsonl")
SERVICE_ACCOUNT = Path("/var/run/secrets/kubernetes.io/serviceaccount")
PROJECT = "local"
ENVIRONMENT = "test"
TRACE_ID = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
SPAN_ID = "bbbbbbbbbbbbbbbb"
SERVICE_ATTRIBUTES = [
    {"key": "service.name", "value": {"stringValue": "sift-acceptance"}},
    {"key": "deployment.environment.name", "value": {"stringValue": ENVIRONMENT}},
]


def write_fixture():
    pod_uid = os.environ["POD_UID"]
    trace_id = uuid.UUID(pod_uid).hex
    event = {
        "schema": "axiom.service.log.v1",
        "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "severity": "INFO",
        "service": {"name": "sift-gke-smoke", "version": "1"},
        "event": "durable_log",
        "message": f"sift-gke-smoke-{pod_uid}",
        "trace_id": trace_id,
        "span_id": trace_id[:16],
        "trace_flags": "01",
        "attributes": {},
    }
    SOURCE.write_text(json.dumps(event, separators=(",", ":")) + "\n", encoding="utf-8")


def read_checkpoint(event, source_size):
    checkpoint = json.loads(Path("checkpoint.json").read_bytes())
    expected = {
        "schema": "collector.checkpoint.v1",
        "source_id": event["message"],
        "offset": source_size,
        "line": 1,
        "accepted": 1,
        "duplicates": 0,
        "rejected": 0,
    }
    if any(checkpoint.get(key) != value for key, value in expected.items()):
        raise RuntimeError("collector checkpoint did not acknowledge the exact fixture")
    rejected = Path("rejected.jsonl")
    if rejected.exists() and rejected.stat().st_size:
        raise RuntimeError("collector rejected fixture data")


def read_record(url, event):
    request = urllib.request.Request(
        f"{url}/api/v1/query",
        data=json.dumps({
            "version": 1, "project": "local", "environment": "test",
            "mode": "sync", "limit": 2,
            "signal": {"kind": "logs", "filter": {
                "op": "eq", "field": "trace_id", "value": event["trace_id"],
            }},
        }).encode(),
        headers={"Content-Type": "application/json"},
    )
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(request, timeout=3) as response:
                records = json.load(response)["data"]["records"]
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            records = []
        if records:
            record = records[0]
            if (
                len(records) != 1
                or record.get("body_text") != event["message"]
                or record.get("trace_id") != event["trace_id"]
                or record.get("span_id") != event["span_id"]
                or record.get("resource", {}).get("service.name") != "sift-gke-smoke"
                or record.get("attributes", {}).get("event.name", {}).get("value") != "durable_log"
            ):
                raise RuntimeError("Sift returned data that does not match the unique fixture")
            return json.dumps(record, sort_keys=True)
        time.sleep(2)
    raise RuntimeError("Sift query did not return the fixture")


def kube_request(method, resource, name, uid=None):
    namespace = (SERVICE_ACCOUNT / "namespace").read_text().strip()
    token = (SERVICE_ACCOUNT / "token").read_text().strip()
    context = ssl.create_default_context(cafile=SERVICE_ACCOUNT / "ca.crt")
    connection = http.client.HTTPSConnection("kubernetes.default.svc", context=context, timeout=5)
    path = f"/api/v1/namespaces/{namespace}/{resource}/{name}"
    headers = {"Authorization": f"Bearer {token}", "Accept": "application/json"}
    body = None
    if uid is not None:
        headers["Content-Type"] = "application/json"
        body = json.dumps({"apiVersion": "v1", "kind": "DeleteOptions", "preconditions": {"uid": uid}})
    try:
        connection.request(method, path, body=body, headers=headers)
        response = connection.getresponse()
        if method == "GET" and response.status == 404:
            return None
        if not 200 <= response.status < 300:
            raise RuntimeError(f"Kubernetes {method} {resource}/{name} returned HTTP {response.status}")
        if method == "GET":
            return json.load(response)
        return None
    finally:
        connection.close()


def image_id(pod):
    return next((
        status.get("imageID")
        for status in pod.get("status", {}).get("containerStatuses", [])
        if status["name"] == "sift"
    ), None)


def wait_for_replacement(api, original_uid, image):
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        pod = api("GET", "pods", POD)
        if pod is not None:
            uid = pod["metadata"]["uid"]
            ready = any(
                condition["type"] == "Ready" and condition["status"] == "True"
                for condition in pod.get("status", {}).get("conditions", [])
            )
            if uid != original_uid and not pod["metadata"].get("deletionTimestamp") and ready:
                if image_id(pod) != image:
                    raise RuntimeError("replacement Sift pod does not use the original image")
                return uid
        time.sleep(2)
    raise RuntimeError("replacement Sift pod did not become ready")


def bound_claim(api):
    claim = api("GET", "persistentvolumeclaims", CLAIM)
    if claim is None or claim.get("status", {}).get("phase") != "Bound":
        raise RuntimeError("Sift PVC is not bound")
    return claim["metadata"]["uid"], claim["spec"]["volumeName"]


def http_call(url, method, path, body=None, headers=None, timeout=10):
    request = urllib.request.Request(f"{url}{path}", data=body, method=method, headers=headers or {})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.read()


def parse_json(raw):
    return json.loads(raw) if raw else None


def post_json(url, path, payload, headers=None):
    merged = {"Content-Type": "application/json", "x-sift-project": PROJECT}
    merged.update(headers or {})
    status, raw = http_call(url, "POST", path, body=json.dumps(payload).encode(), headers=merged)
    return status, parse_json(raw)


def pb_varint(n):
    out = bytearray()
    while True:
        byte = n & 0x7F
        n >>= 7
        if n:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def pb_field(number, wire_type, payload):
    return pb_varint(number << 3 | wire_type) + payload


def pb_message(number, data):
    return pb_field(number, 2, pb_varint(len(data)) + data)


def pb_string(number, text):
    return pb_message(number, text.encode())


def pb_any_string(text):
    return pb_string(1, text)


def pb_keyvalue(key, text):
    return pb_string(1, key) + pb_message(2, pb_any_string(text))


def snappy_raw(data):
    """A snappy raw block that holds literals only."""
    out = bytearray(pb_varint(len(data)))
    for start in range(0, len(data), 65536):
        chunk = data[start:start + 65536]
        if len(chunk) <= 60:
            out.append((len(chunk) - 1) << 2)
        else:
            out.append(61 << 2)
            out += struct.pack("<H", len(chunk) - 1)
        out += chunk
    return bytes(out)


def otlp_logs_protobuf(timestamp_nanos, event_id):
    resource = (
        pb_message(1, pb_keyvalue("service.name", "sift-acceptance"))
        + pb_message(1, pb_keyvalue("deployment.environment.name", ENVIRONMENT))
    )
    record = (
        pb_field(1, 1, struct.pack("<Q", timestamp_nanos))
        + pb_field(2, 0, pb_varint(9))
        + pb_string(3, "INFO")
        + pb_message(5, pb_any_string("protobuf accepted"))
        + pb_message(6, pb_keyvalue("sift.event_id", event_id))
    )
    resource_logs = pb_message(1, resource) + pb_message(2, pb_message(2, record))
    return pb_message(1, resource_logs)


def remote_write_v1(timestamp_millis):
    def label(name, value):
        return pb_string(1, name) + pb_string(2, value)

    series = (
        pb_message(1, label("__name__", "sift_acceptance_total"))
        + pb_message(1, label("environment", ENVIRONMENT))
        + pb_message(1, label("fixture", "smoke-remote-write"))
        + pb_message(1, label("project", PROJECT))
        + pb_message(2, pb_field(1, 1, struct.pack("<d", 1.0)) + pb_field(2, 0, pb_varint(timestamp_millis)))
    )
    return snappy_raw(pb_message(1, series))


def retry(check, seconds=30):
    """Call check every 2 s until it returns a truthy value; False when time is up."""
    for attempt in range(seconds // 2 + 1):
        if check():
            return True
        if attempt < seconds // 2:
            time.sleep(2)
    return False


def fail(name, detail):
    raise RuntimeError(f"{name}: {detail}")


def as_int(value):
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def is_plain_ack(status, data):
    return status == 200 and isinstance(data, dict) and data.get("partialSuccess") is None


def metric_query(name):
    return {
        "version": 1, "project": PROJECT, "environment": ENVIRONMENT,
        "signal": {"kind": "metrics", "name": name, "function": "sum",
                   "step_seconds": 1, "group_by": ["service.name"]},
        "limit": 10, "mode": "sync",
    }


def series_count(url):
    _, data = post_json(url, "/api/v1/query", metric_query("sift_acceptance_total"))
    return len((((data or {}).get("data") or {}).get("series")) or [])


def metric_series_count(url):
    state = {}

    def ready():
        state["count"] = series_count(url)
        return state["count"] >= 1

    if not retry(ready):
        fail("remote_write_v2_rejected", "no sift_acceptance_total series before the 2.0 request")
    return state["count"]


def otlp_logs_partial(url, now):
    payload = {"resourceLogs": [{"resource": {"attributes": SERVICE_ATTRIBUTES}, "scopeLogs": [{"logRecords": [
        {"timeUnixNano": str(now), "severityText": "ERROR", "traceId": TRACE_ID, "spanId": SPAN_ID,
         "body": {"stringValue": "smoke accepted"},
         "attributes": [{"key": "sift.event_id", "value": {"stringValue": "smoke-log"}}]},
        {"timeUnixNano": str(now), "body": None},
    ]}]}]}
    status, data = post_json(url, "/v1/logs", payload)
    partial = (data or {}).get("partialSuccess") or {}
    if status != 200 or as_int(partial.get("rejectedLogRecords")) != 1 \
            or "body is required" not in str(partial.get("errorMessage", "")):
        fail("otlp_logs_partial", f"expected HTTP 200 with one rejected record saying 'body is required', got {status} {data}")


def otlp_metrics(url, now):
    payload = {"resourceMetrics": [{"resource": {"attributes": SERVICE_ATTRIBUTES}, "scopeMetrics": [{"metrics": [{
        "name": "sift.acceptance.gauge", "unit": "1",
        "gauge": {"dataPoints": [{
            "timeUnixNano": str(now + 10), "asDouble": 42.5,
            "attributes": [{"key": "fixture", "value": {"stringValue": "smoke"}}],
            "exemplars": [{"timeUnixNano": str(now + 10), "asDouble": 42.5,
                           "traceId": TRACE_ID, "spanId": SPAN_ID}],
        }]},
    }]}]}]}
    status, data = post_json(url, "/v1/metrics", payload)
    if not is_plain_ack(status, data):
        fail("otlp_metrics", f"expected HTTP 200 and no partialSuccess, got {status} {data}")


def otlp_traces(url, now):
    payload = {"resourceSpans": [{"resource": {"attributes": SERVICE_ATTRIBUTES}, "scopeSpans": [{"spans": [{
        "traceId": TRACE_ID, "spanId": SPAN_ID, "name": "GET /smoke",
        "startTimeUnixNano": str(now + 20), "endTimeUnixNano": str(now + 1000020),
        "status": {"code": 2},
        "attributes": [{"key": "http.route", "value": {"stringValue": "/smoke"}}],
    }]}]}]}
    status, data = post_json(url, "/v1/traces", payload)
    if not is_plain_ack(status, data):
        fail("otlp_traces", f"expected HTTP 200 and no partialSuccess, got {status} {data}")


def otlp_logs_gzip(url, now):
    payload = {"resourceLogs": [{"resource": {"attributes": SERVICE_ATTRIBUTES[:1]}, "scopeLogs": [{"logRecords": [{
        "timeUnixNano": str(now + 30), "body": {"stringValue": "gzip accepted"},
        "attributes": [{"key": "sift.event_id", "value": {"stringValue": "smoke-gzip"}}],
    }]}]}]}
    status, raw = http_call(url, "POST", "/v1/logs", body=gzip.compress(json.dumps(payload).encode()),
                            headers={"Content-Type": "application/json", "content-encoding": "gzip",
                                     "x-sift-project": PROJECT})
    if not is_plain_ack(status, parse_json(raw)):
        fail("otlp_logs_gzip", f"expected HTTP 200 and no partialSuccess, got {status} {raw[:200]!r}")


def otlp_logs_binary(url, now):
    status, _ = http_call(url, "POST", "/v1/logs", body=otlp_logs_protobuf(now + 40, "smoke-protobuf-0"),
                          headers={"Content-Type": "application/x-protobuf", "x-sift-project": PROJECT})
    if status != 200:
        fail("otlp_logs_protobuf", f"expected HTTP 200, got {status}")


def remote_write_v1_accepted(url, now):
    status, _ = http_call(url, "POST", "/prometheus/api/v1/write", body=remote_write_v1((now + 50) // 1_000_000),
                          headers={"Content-Type": "application/x-protobuf", "content-encoding": "snappy",
                                   "x-prometheus-remote-write-version": "0.1.0", "x-sift-project": PROJECT})
    if status != 204:
        fail("remote_write_v1", f"expected 204, got {status}")


def remote_write_v2_rejected(url):
    before = metric_series_count(url)
    status, _ = http_call(url, "POST", "/prometheus/api/v1/write", body=b"",
                          headers={"Content-Type": "application/x-protobuf;proto=io.prometheus.write.v2.Request",
                                   "content-encoding": "snappy", "x-prometheus-remote-write-version": "2.0.0",
                                   "x-sift-project": PROJECT})
    if status != 415:
        fail("remote_write_v2_rejected", f"expected 415, got {status}")
    after = series_count(url)
    if after != before:
        fail("remote_write_v2_rejected", f"the rejected write changed the series count from {before} to {after}")


def query_logs(url):
    payload = {
        "version": 1, "project": PROJECT, "environment": ENVIRONMENT,
        "signal": {"kind": "logs", "filter": {"op": "regex", "field": "body_text", "pattern": "smoke.*accepted"}},
        "limit": 10, "mode": "sync",
    }

    def ready():
        status, data = post_json(url, "/api/v1/query", payload)
        records = (((data or {}).get("data")) or {}).get("records") or []
        matches = [record for record in records if record.get("event_id") == "smoke-log"]
        return status == 200 and len(matches) == 1 and (data or {}).get("partial") is False

    if not retry(ready):
        fail("query_logs", "no single smoke-log record with partial false")


def query_metrics(url):
    def ready():
        status, data = post_json(url, "/api/v1/query", metric_query("sift.acceptance.gauge"))
        series = (((data or {}).get("data")) or {}).get("series") or []
        return status == 200 and len(series) == 1 and series[0].get("aggregate") == 42.5

    if not retry(ready):
        fail("query_metrics", "expected one series with sum 42.5")


def query_traces(url):
    payload = {
        "version": 1, "project": PROJECT, "environment": ENVIRONMENT,
        "signal": {"kind": "traces", "service": "sift-acceptance", "operation": "GET /smoke",
                   "min_duration_ms": 1},
        "limit": 10, "mode": "sync",
    }

    def ready():
        status, data = post_json(url, "/api/v1/query", payload)
        traces = (((data or {}).get("data")) or {}).get("traces") or []
        return status == 200 and sum(trace.get("trace_id") == TRACE_ID for trace in traces) == 1

    if not retry(ready):
        fail("query_traces", "no trace with the fixture trace id")


def trace_read(url):
    def ready():
        status, raw = http_call(url, "GET", f"/api/v1/traces/{TRACE_ID}?project={PROJECT}",
                                headers={"x-sift-project": PROJECT})
        data = parse_json(raw) if status == 200 else None
        return isinstance(data, dict) and data.get("trace_id") == TRACE_ID \
            and len(data.get("spans") or []) == 1 and len(data.get("critical_path_span_ids") or []) == 1

    if not retry(ready):
        fail("trace_read", "expected the fixture trace with one span and one critical-path span")


def correlate(url):
    payload = {"version": 1, "project": PROJECT, "environment": ENVIRONMENT, "trace_id": TRACE_ID, "limit": 10}

    def ready():
        status, data = post_json(url, "/api/v1/correlate", payload)
        data = data or {}
        logs = data.get("logs") or []
        return status == 200 and sum(log.get("event_id") == "smoke-log" for log in logs) == 1 \
            and len(data.get("traces") or []) == 1 and data.get("partial") is False

    if not retry(ready):
        fail("correlate", "expected one smoke-log record, one trace and partial false")


def log_tail(url):
    payload = {
        "version": 1, "project": PROJECT, "environment": ENVIRONMENT,
        "filter": {"op": "eq", "field": "event_id", "value": "smoke-log"}, "wait_ms": 1000, "limit": 10,
    }

    def ready():
        status, data = post_json(url, "/api/v1/logs/tail", payload)
        records = (((data or {}).get("data")) or {}).get("records") or []
        return status == 200 and sum(record.get("event_id") == "smoke-log" for record in records) == 1

    if not retry(ready):
        fail("log_tail", "expected one smoke-log record in the tail")


def check_protocols(url, now_nanos=None):
    """Run the OTLP, Remote Write and read checks in order; raise on the first failure."""
    now = time.time_ns() if now_nanos is None else now_nanos
    steps = [
        ("otlp_logs_partial", lambda: otlp_logs_partial(url, now)),
        ("otlp_metrics", lambda: otlp_metrics(url, now)),
        ("otlp_traces", lambda: otlp_traces(url, now)),
        ("otlp_logs_gzip", lambda: otlp_logs_gzip(url, now)),
        ("otlp_logs_protobuf", lambda: otlp_logs_binary(url, now)),
        ("remote_write_v1", lambda: remote_write_v1_accepted(url, now)),
        ("remote_write_v2_rejected", lambda: remote_write_v2_rejected(url)),
        ("query_logs", lambda: query_logs(url)),
        ("query_metrics", lambda: query_metrics(url)),
        ("query_traces", lambda: query_traces(url)),
        ("trace_read", lambda: trace_read(url)),
        ("correlate", lambda: correlate(url)),
        ("log_tail", lambda: log_tail(url)),
    ]
    results = {}
    for name, step in steps:
        step()
        results[name] = "passed"
    return results


def verify(api, url):
    source = SOURCE.read_bytes()
    event = json.loads(source)
    read_checkpoint(event, len(source))

    pod = api("GET", "pods", POD)
    if pod is None or not image_id(pod):
        raise RuntimeError("Sift pod or image identity is missing")
    original_uid = pod["metadata"]["uid"]
    image = image_id(pod)
    claim_uid, volume = bound_claim(api)
    before = read_record(url, event)
    protocols = check_protocols(url)

    api("DELETE", "pods", POD, uid=original_uid)
    replacement_uid = wait_for_replacement(api, original_uid, image)
    if bound_claim(api) != (claim_uid, volume):
        raise RuntimeError("replacement Sift pod has a different PVC or volume")
    if read_record(url, event) != before:
        raise RuntimeError("fixture record changed after the Sift pod restart")

    print(json.dumps({
        "records": 1, "marker": event["message"], "original_pod_uid": original_uid,
        "replacement_pod_uid": replacement_uid, "pvc_uid": claim_uid, "volume": volume,
        "image_id": image, "result": "PASS", "protocols": protocols,
    }))


if __name__ == "__main__":
    if sys.argv[1:] == ["fixture"]:
        write_fixture()
    elif sys.argv[1:] == ["verify"]:
        verify(kube_request, os.environ["SIFT_URL"])
    else:
        sys.exit("usage: smoke.py fixture|verify")
