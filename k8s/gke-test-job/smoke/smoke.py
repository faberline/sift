"""Generate one structured log and verify its recovery after a pod restart."""

import http.client
import json
import os
import ssl
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

    api("DELETE", "pods", POD, uid=original_uid)
    replacement_uid = wait_for_replacement(api, original_uid, image)
    if bound_claim(api) != (claim_uid, volume):
        raise RuntimeError("replacement Sift pod has a different PVC or volume")
    if read_record(url, event) != before:
        raise RuntimeError("fixture record changed after the Sift pod restart")

    print(json.dumps({
        "records": 1, "marker": event["message"], "original_pod_uid": original_uid,
        "replacement_pod_uid": replacement_uid, "pvc_uid": claim_uid, "volume": volume,
        "image_id": image, "result": "PASS",
    }))


if __name__ == "__main__":
    if sys.argv[1:] == ["fixture"]:
        write_fixture()
    elif sys.argv[1:] == ["verify"]:
        verify(kube_request, os.environ["SIFT_URL"])
    else:
        sys.exit("usage: smoke.py fixture|verify")
