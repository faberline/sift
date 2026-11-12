"""One restore drill for faberline/sift#93: back up a live Sift, restore it, compare.

Scope: the single-voter path only. The StatefulSet runs `sift serve --role all`
with SIFT_AUTH=off. This drill runs `sift backup --url ... --dest gs://...` and
`sift restore --data-dir ... --source gs://...`. It does NOT test the
three-voter `restore-complete` path: the test cluster pools cannot fit three
voters.

Usage in the Job: `drill.py fixture` (init container) writes 200 records;
`drill.py verify` (main container) counts them, backs up, replaces the PVC,
restores the backup into it, and compares the records again.

The helpers `digest`, `compare` and `pod_spec` have no I/O.
"""

import copy
import hashlib
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


SERVICE_ACCOUNT = Path("/var/run/secrets/kubernetes.io/serviceaccount")
LOG_ACCEPT = "text/plain, */*"
SOURCE = Path("event.jsonl")
SERVICE_NAME = "sift-restore-drill"
SERVICE_URL = "http://sift-restore-drill:7380"
STATEFULSET = "sift-restore-drill"
POD = "sift-restore-drill-0"
CLAIM = "data-sift-restore-drill-0"
BACKUP_POD = "sift-restore-drill-backup"
RESTORE_POD = "sift-restore-drill-restore"
POD_SA = "sift-restore-drill"
BUCKET = "gs://sift-restore-drill-20261111"
RECORDS = 200
POOL_LABEL = "faberline.dev/pool"
PROBE_LABEL = {"app.kubernetes.io/name": "sift-restore-drill-probe"}
RESTORE_LABEL = {"app.kubernetes.io/name": "sift-restore-drill-restore"}
PYTHON_IMAGE = "docker.io/library/python:3.13-alpine@sha256:d76f585fb0d5107dd47143da5e61554fd9cae656a30f3470f815cb8475769b6a"

# Copied byte for byte from k8s/overlays/gke-restore-drill/sift.yaml.
PREPARE_DATA_ROOT = {
    "name": "prepare-data-root",
    "image": PYTHON_IMAGE,
    "command": [
        "/bin/sh",
        "-ec",
        "mkdir -p /var/lib/sift/data\n"
        "chown 65532:65532 /var/lib/sift/data\n"
        "chmod 0700 /var/lib/sift/data\n"
        "test \"$(stat -c '%u:%g:%a' /var/lib/sift/data)\" = '65532:65532:700'\n",
    ],
    "securityContext": {
        "runAsNonRoot": False,
        "runAsUser": 0,
        "runAsGroup": 0,
        "allowPrivilegeEscalation": False,
        "readOnlyRootFilesystem": True,
        "capabilities": {"drop": ["ALL"], "add": ["CHOWN", "FOWNER"]},
    },
    "resources": {"requests": {"cpu": "10m", "memory": "16Mi"}},
    "volumeMounts": [{"name": "data", "mountPath": "/var/lib/sift"}],
}


def digest(bodies):
    return hashlib.sha256("\n".join(sorted(bodies)).encode()).hexdigest()


def compare(before, after):
    """Same count and same sorted content. A missing or changed record fails."""
    return len(before) == len(after) and digest(before) == digest(after)


def write_fixture():
    """Write RECORDS unique log lines to ./event.jsonl (the /work emptyDir)."""
    pod_uid = uuid.UUID(os.environ["POD_UID"])
    lines = []
    for i in range(RECORDS):
        trace_id = uuid.uuid5(pod_uid, str(i)).hex
        event = {
            "schema": "axiom.service.log.v1",
            "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "severity": "INFO",
            "service": {"name": SERVICE_NAME, "version": "1"},
            "event": "durable_log",
            "message": f"sift-restore-drill-{i}",
            "trace_id": trace_id,
            "span_id": trace_id[:16],
            "trace_flags": "01",
            "attributes": {},
        }
        lines.append(json.dumps(event, separators=(",", ":")))
    SOURCE.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _with_security(container):
    """Add the restricted container defaults unless the container sets its own."""
    result = copy.deepcopy(container)
    result.setdefault("securityContext", {
        "allowPrivilegeEscalation": False,
        "readOnlyRootFilesystem": True,
        "capabilities": {"drop": ["ALL"]},
    })
    return result


def pod_spec(name, pool, sa, labels, containers, volumes=None, init_containers=None):
    """Full Pod object with the placement fields that test-cluster.md requires."""
    spec = {
        "serviceAccountName": sa,
        "automountServiceAccountToken": False,
        "restartPolicy": "Never",
        "nodeSelector": {POOL_LABEL: pool},
        "tolerations": [{"key": POOL_LABEL, "operator": "Equal", "value": pool, "effect": "NoSchedule"}],
        "imagePullSecrets": [{"name": "ghcr-pull"}],
        "securityContext": {
            "runAsNonRoot": True,
            "runAsUser": 65532,
            "runAsGroup": 65532,
            "seccompProfile": {"type": "RuntimeDefault"},
        },
        "containers": [_with_security(container) for container in containers],
    }
    if init_containers:
        spec["initContainers"] = [_with_security(container) for container in init_containers]
    if volumes:
        spec["volumes"] = copy.deepcopy(volumes)
    return {
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": name,
            "labels": {**labels, "app.kubernetes.io/part-of": "sift-restore-drill"},
        },
        "spec": spec,
    }


def namespace():
    return (SERVICE_ACCOUNT / "namespace").read_text().strip()


def core(resource, name=""):
    path = f"/api/v1/namespaces/{namespace()}/{resource}"
    return f"{path}/{name}" if name else path


def kube_api(method, path, body=None, content_type="application/json"):
    """Call the Kubernetes API with the pod's token. Returns parsed JSON, or the
    text for a `/log` path, which accepts `text/plain, */*`. Returns None for HTTP 404
    on GET. Other non-2xx raise, and the error carries the first 500 characters of the body."""
    token = (SERVICE_ACCOUNT / "token").read_text().strip()
    context = ssl.create_default_context(cafile=SERVICE_ACCOUNT / "ca.crt")
    connection = http.client.HTTPSConnection("kubernetes.default.svc", context=context, timeout=10)
    is_log = path.endswith("/log")
    headers = {
        "Authorization": f"Bearer {token}",
        "Accept": LOG_ACCEPT if is_log else "application/json",
    }
    payload = None
    if body is not None:
        headers["Content-Type"] = content_type
        payload = json.dumps(body)
    try:
        connection.request(method, path, body=payload, headers=headers)
        response = connection.getresponse()
        text = response.read().decode()
        if method == "GET" and response.status == 404:
            return None
        if not 200 <= response.status < 300:
            raise RuntimeError(f"Kubernetes {method} {path} returned HTTP {response.status}: {text[:500]}")
        if is_log:
            return text
        return json.loads(text) if text else None
    finally:
        connection.close()


def sift_image(api):
    """The digest-pinned image that kustomize wrote into the StatefulSet."""
    sts = api("GET", f"/apis/apps/v1/namespaces/{namespace()}/statefulsets/{STATEFULSET}")
    for container in sts["spec"]["template"]["spec"]["containers"]:
        if container["name"] == "sift":
            return container["image"]
    raise RuntimeError("StatefulSet has no container named sift")


def wait_until(check, timeout, what):
    deadline = time.monotonic() + timeout
    while True:
        result = check()
        if result:
            return result
        if time.monotonic() >= deadline:
            raise RuntimeError(f"timed out waiting for {what}")
        time.sleep(2)


def wait_pod_phase(api, name, phase, timeout):
    def check():
        pod = api("GET", core("pods", name))
        current = (pod or {}).get("status", {}).get("phase")
        if current == "Failed":
            raise RuntimeError(f"pod {name} failed")
        return current == phase
    wait_until(check, timeout, f"pod {name} to reach {phase}")


def wait_gone(api, path, timeout, what):
    wait_until(lambda: api("GET", path) is None, timeout, what)


def scale(api, replicas):
    api(
        "PATCH",
        f"/apis/apps/v1/namespaces/{namespace()}/statefulsets/{STATEFULSET}/scale",
        {"spec": {"replicas": replicas}},
        content_type="application/merge-patch+json",
    )


def wait_ready(url, timeout=300):
    def check():
        try:
            with urllib.request.urlopen(f"{url}/readyz", timeout=3) as response:
                return response.status == 200
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            return False
    wait_until(check, timeout, "Sift /readyz")


def fetch_bodies(url):
    """body_text of every drill record in the query result. [] on a transport error."""
    request = urllib.request.Request(
        f"{url}/api/v1/query",
        data=json.dumps({
            "version": 1, "project": "local", "environment": "test",
            "mode": "sync", "limit": 500, "signal": {"kind": "logs"},
        }).encode(),
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            records = json.load(response)["data"]["records"]
    except (urllib.error.URLError, TimeoutError, ConnectionError):
        return []
    return [
        record["body_text"]
        for record in records
        if record.get("resource", {}).get("service.name") == SERVICE_NAME
    ]


def query_bodies(url, timeout=60):
    """Poll for up to 60 s until RECORDS drill records are visible. Returns what it saw last."""
    deadline = time.monotonic() + timeout
    while True:
        bodies = fetch_bodies(url)
        if len(bodies) == RECORDS or time.monotonic() >= deadline:
            return bodies
        time.sleep(2)


def run_backup(api, image, run_id):
    """Back up the live Sift with `sift backup` in a Pod. Returns the gs:// URI it wrote."""
    dest = f"{BUCKET}/{run_id}/backup"
    api("POST", core("pods"), pod_spec(
        BACKUP_POD, "sift-client", POD_SA, PROBE_LABEL,
        [{
            "name": "sift",
            "image": image,
            "args": ["backup", "--url", SERVICE_URL, "--dest", dest, "--retention-secs", "86400"],
            "resources": {"requests": {"cpu": "100m", "memory": "128Mi"}},
        }],
    ))
    wait_pod_phase(api, BACKUP_POD, "Succeeded", 300)
    report = json.loads(api("GET", core("pods", BACKUP_POD) + "/log"))
    api("DELETE", core("pods", BACKUP_POD))
    return f"{BUCKET}/" + report["object"]["key"]


def replace_claim(api):
    """Stop the StatefulSet, delete its PVC and create a new empty one. Returns (old uid, new uid)."""
    scale(api, 0)
    wait_gone(api, core("pods", POD), 120, f"pod {POD} to stop")
    old = api("GET", core("persistentvolumeclaims", CLAIM))
    if old is None:
        raise RuntimeError(f"PVC {CLAIM} is missing before the restore")
    api("DELETE", core("persistentvolumeclaims", CLAIM))
    wait_gone(api, core("persistentvolumeclaims", CLAIM), 120, f"PVC {CLAIM} to delete")
    api("POST", core("persistentvolumeclaims"), {
        "apiVersion": "v1",
        "kind": "PersistentVolumeClaim",
        "metadata": {"name": CLAIM, "labels": {"app.kubernetes.io/name": STATEFULSET}},
        "spec": {
            "accessModes": ["ReadWriteOnce"],
            "storageClassName": "dynamic-rwo",
            "resources": {"requests": {"storage": "4Gi"}},
        },
    })
    return old["metadata"]["uid"]


def run_restore(api, image, source):
    """Restore the backup into the new PVC in a Pod, then bring Sift back up."""
    api("POST", core("pods"), pod_spec(
        RESTORE_POD, "sift-server", POD_SA, RESTORE_LABEL,
        [{
            "name": "sift",
            "image": image,
            "args": ["restore", "--data-dir", "/var/lib/sift/data", "--source", source],
            "resources": {"requests": {"cpu": "100m", "memory": "128Mi"}},
            "volumeMounts": [{"name": "data", "mountPath": "/var/lib/sift"}],
        }],
        volumes=[{"name": "data", "persistentVolumeClaim": {"claimName": CLAIM}}],
        init_containers=[PREPARE_DATA_ROOT],
    ))
    wait_pod_phase(api, RESTORE_POD, "Succeeded", 300)
    api("DELETE", core("pods", RESTORE_POD))
    wait_gone(api, core("pods", RESTORE_POD), 120, f"pod {RESTORE_POD} to delete")


def drill(api, url, run_id):
    before = query_bodies(url)
    print(f"BEFORE count={len(before)} sha256={digest(before)}", flush=True)
    if len(before) != RECORDS:
        print(f"FAIL: expected {RECORDS} records before the backup, got {len(before)}", file=sys.stderr)
        return 1

    image = sift_image(api)
    source = run_backup(api, image, run_id)
    old_uid = replace_claim(api)
    run_restore(api, image, source)

    scale(api, 1)
    wait_ready(url)
    new = api("GET", core("persistentvolumeclaims", CLAIM))
    if new is None or new.get("status", {}).get("phase") != "Bound" or new["metadata"]["uid"] == old_uid:
        raise RuntimeError("the restored PVC is not a new Bound claim")
    new_uid = new["metadata"]["uid"]

    after = query_bodies(url)
    print(f"AFTER count={len(after)} sha256={digest(after)}", flush=True)
    if len(after) != RECORDS or not compare(before, after):
        print("FAIL: restored records differ from the records before the backup", file=sys.stderr)
        return 1

    print(json.dumps({
        "records": RECORDS,
        "before": digest(before),
        "after": digest(after),
        "source": source,
        "pvc_old": old_uid,
        "pvc_new": new_uid,
        "result": "PASS",
    }), flush=True)
    return 0


def verify(api, url):
    run_id = os.environ["POD_UID"]
    try:
        return drill(api, url, run_id)
    finally:
        print(
            f"CLEANUP objects under {run_id}/ stay until the bucket rule deletes them; "
            "the desk removes the bucket",
            flush=True,
        )


def main(argv):
    if argv == ["fixture"]:
        write_fixture()
        return 0
    if argv == ["verify"]:
        return verify(kube_api, os.environ.get("SIFT_URL", SERVICE_URL))
    sys.exit("usage: drill.py fixture|verify")


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
