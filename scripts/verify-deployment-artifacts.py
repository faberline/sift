#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["PyYAML==6.0.3", "jsonschema==4.26.0"]
# ///
"""Check Sift's local deployment artifacts with Cargo, kubectl, PyYAML, and jsonschema.

Use `uv run --script scripts/verify-deployment-artifacts.py` for the pinned Python dependencies.

The image digest and cloud values are fixtures. This check does not build or
pull containers, deploy workloads, or test API-server admission.
"""

import copy
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

from jsonschema import Draft7Validator
import yaml


ROOT = Path(__file__).resolve().parents[1]
IMAGE_NAME = "example.invalid/sift"
DIGEST = "sha256:" + hashlib.sha256(b"sift-artifact-check-only").hexdigest()
IMAGE = f"{IMAGE_NAME}@{DIGEST}"
OPERATOR_NAMESPACE = "sift-artifact-system"
PROFILES = {
    "dev": ("sift", "default", "off"),
    "staging": ("sift", "staging", "kubernetes"),
    "prod": ("sift", "production", "kubernetes"),
    "template": ("sift-artifact", "sift-artifact", "kubernetes"),
}
FIXTURES = {
    "REPLACE_ME__SIFT_IMAGE_DIGEST": IMAGE,
    "REPLACE_ME__SIFT_PEER_TLS_SECRET": "sift-artifact-peer-tls",
    "REPLACE_ME__GCS_ARCHIVE_URI": "gs://sift-artifact-fixture/archive",
    "REPLACE_ME__OPTIONAL_ARCHIVE_MANIFEST_URI": "gs://sift-artifact-fixture/archive/manifest.json",
    "REPLACE_ME__GCP_PROJECT_ID": "sift-artifact-fixture",
    "REPLACE_ME__GKE_CLUSTER_NAME": "sift-artifact-cluster",
    "REPLACE_ME__GKE_LOCATION": "asia-east1-a",
    "REPLACE_ME__NAME": "sift-artifact",
    "REPLACE_ME__NAMESPACE": "sift-artifact",
    "REPLACE_ME__SIFT_PUBLIC_HOST": "sift.example.invalid",
    "REPLACE_ME__SIFT_PUBLIC_STATIC_IP_NAME": "sift-artifact-ip",
}


def run(*args, env=None):
    process = subprocess.run(args, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE)
    if process.returncode:
        print(process.stdout, end="", file=sys.stderr)
        process.check_returncode()
    return process.stdout


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def objects(text, source):
    result = {}
    for item in yaml.safe_load_all(text):
        require(isinstance(item, dict) and item.get("apiVersion"), f"{source}: invalid resource")
        key = item["kind"], item["metadata"]["name"]
        require(key not in result, f"{source}: duplicate resource {key}")
        result[key] = item
    require(result, f"{source}: no resources")
    return result


def check_public_tls(manifests, profile):
    if profile != "prod":
        return
    ingress = manifests.get(("Ingress", "sift-public"))
    require(ingress is not None, "prod: public API has no Ingress")
    certificate = manifests.get(("ManagedCertificate", "sift-public"))
    require(certificate is not None, "prod: public API has no managed certificate")
    annotations = ingress["metadata"].get("annotations", {})
    require(annotations.get("kubernetes.io/ingress.allow-http") == "false"
            and annotations.get("networking.gke.io/managed-certificates") == "sift-public",
            "prod: public API allows plain HTTP or has no certificate")
    rule = ingress["spec"]["rules"][0]
    backend = rule["http"]["paths"][0]["backend"]["service"]
    require(backend == {"name": "sift-gateway", "port": {"number": 7380}},
            "prod: public Ingress does not reach the gateway Service")
    require(certificate["spec"]["domains"] == [rule["host"]],
            "prod: certificate domain differs from the Ingress host")


def validate_overlay(text, profile):
    manifests = objects(text, profile)
    check_public_tls(manifests, profile)
    name, namespace, auth = PROFILES[profile]
    crd = manifests["CustomResourceDefinition", "sifts.sift.axiom.dev"]
    require(crd["apiVersion"] == "apiextensions.k8s.io/v1", "unexpected Sift CRD API version")
    instance = manifests["Sift", name]
    group, version = instance["apiVersion"].split("/")
    require(group == crd["spec"]["group"], f"{profile}: Sift API group differs from the CRD")
    schema, = [entry["schema"]["openAPIV3Schema"]["properties"]["spec"]
               for entry in crd["spec"]["versions"] if entry["name"] == version and entry["served"]]
    Draft7Validator.check_schema(schema)
    Draft7Validator(schema).validate(instance["spec"])
    require(instance["metadata"]["namespace"] == namespace and instance["spec"]["auth"] == auth,
            f"{profile}: wrong environment settings")
    require(instance["spec"]["image"] == IMAGE, f"{profile}: wrong instance image digest")

    manifests["Namespace", OPERATOR_NAMESPACE]
    account = manifests["ServiceAccount", "sift-operator"]
    deployment = manifests["Deployment", "sift-operator"]
    binding = manifests["ClusterRoleBinding", "sift-operator"]
    role = manifests["ClusterRole", "sift-operator"]
    pod = deployment["spec"]["template"]["spec"]
    operator, = pod["containers"]
    require(deployment["metadata"]["namespace"] == account["metadata"]["namespace"] == OPERATOR_NAMESPACE
            and pod["serviceAccountName"] == account["metadata"]["name"]
            and binding["roleRef"]["kind"] == "ClusterRole"
            and binding["roleRef"]["name"] == role["metadata"]["name"]
            and binding["subjects"] == [{"kind": "ServiceAccount", "name": pod["serviceAccountName"],
                                         "namespace": OPERATOR_NAMESPACE}],
            f"{profile}: operator and RBAC do not agree")
    require(operator["image"] == IMAGE and operator["args"] == ["k8s", "operator", "run"],
            f"{profile}: wrong operator image digest or entrypoint")
    require(any(rule["apiGroups"] == ["networking.k8s.io"] and "networkpolicies" in rule["resources"]
                for rule in role["rules"]), f"{profile}: operator cannot manage network policies")


def check_public_tls_rejects_plain_http():
    host = "sift.example.invalid"
    good = {
        ("Ingress", "sift-public"): {
            "apiVersion": "networking.k8s.io/v1",
            "kind": "Ingress",
            "metadata": {"name": "sift-public", "namespace": "production", "annotations": {
                "kubernetes.io/ingress.class": "gce",
                "networking.gke.io/managed-certificates": "sift-public",
                "kubernetes.io/ingress.allow-http": "false",
                "kubernetes.io/ingress.global-static-ip-name": "sift-artifact-ip",
            }},
            "spec": {"rules": [{"host": host, "http": {"paths": [{
                "path": "/", "pathType": "Prefix",
                "backend": {"service": {"name": "sift-gateway", "port": {"number": 7380}}},
            }]}}]},
        },
        ("ManagedCertificate", "sift-public"): {
            "apiVersion": "networking.gke.io/v1",
            "kind": "ManagedCertificate",
            "metadata": {"name": "sift-public", "namespace": "production"},
            "spec": {"domains": [host]},
        },
    }
    check_public_tls(good, "prod")
    bad_copies = []
    no_ingress = copy.deepcopy(good)
    del no_ingress[("Ingress", "sift-public")]
    bad_copies.append(no_ingress)
    plain = copy.deepcopy(good)
    plain[("Ingress", "sift-public")]["metadata"]["annotations"]["kubernetes.io/ingress.allow-http"] = "true"
    bad_copies.append(plain)
    wrong_port = copy.deepcopy(good)
    wrong_port[("Ingress", "sift-public")]["spec"]["rules"][0]["http"]["paths"][0]["backend"]["service"]["port"] = {"number": 80}
    bad_copies.append(wrong_port)
    for bad in bad_copies:
        try:
            check_public_tls(bad, "prod")
        except SystemExit:
            continue
        raise SystemExit("public TLS check accepts plain HTTP or a missing Ingress")
    print("Public TLS check rejects plain HTTP")


def render(directory, sift, version):
    k8s = directory / "k8s"
    shutil.copytree(ROOT / "k8s", k8s)
    run(sift, "k8s", "crd", "render", "--out", k8s / "crd/sift.yaml")
    run(sift, "k8s", "operator", "render", "--namespace", OPERATOR_NAMESPACE, "--image", IMAGE,
        "--out", k8s / "operator/operator.yaml")
    artifacts = {}
    for profile in PROFILES:
        overlay = k8s / "overlays" / profile
        instance = overlay / "sift.yaml"
        run(sift, "k8s", "instance", "render", "--profile", profile, "--out", instance)
        text = re.sub(r"REPLACE_ME__[A-Z0-9_]+", lambda token: FIXTURES.get(token[0], token[0]),
                      instance.read_text())
        require("REPLACE_" not in text, f"{profile}: unresolved fixture placeholder")
        instance.write_text(text)
        for extra in sorted(overlay.glob("*.yaml")):
            if extra.name in ("kustomization.yaml", "sift.yaml"):
                continue
            extra.write_text(re.sub(r"REPLACE_ME__[A-Z0-9_]+",
                                    lambda token: FIXTURES.get(token[0], token[0]), extra.read_text()))
        text = run("kubectl", "kustomize", overlay)
        validate_overlay(text, profile)
        artifacts[profile] = text

    collector = directory / "collector.yaml"
    run(sift, "k8s", "collector", "render", "--namespace", OPERATOR_NAMESPACE, "--image", IMAGE,
        "--out", collector)
    artifacts[collector.name] = collector.read_text()
    daemon = objects(artifacts[collector.name], collector.name)["DaemonSet", "sift-collector"]
    pod = daemon["spec"]["template"]["spec"]
    container, = pod["containers"]
    require(daemon["metadata"]["namespace"] == OPERATOR_NAMESPACE and container["image"] == IMAGE
            and container["args"][0] == "collect" and pod["automountServiceAccountToken"] is False,
            "wrong collector namespace, image, entrypoint, or token mount")

    smoke = k8s / "overlays/gke-test"
    kustomization = smoke / "kustomization.yaml"
    config = yaml.safe_load(kustomization.read_text())
    config["images"] = [{"name": "sift", "newName": IMAGE_NAME, "digest": DIGEST}]
    kustomization.write_text(yaml.safe_dump(config))
    artifacts["gke-test"] = run("kubectl", "kustomize", smoke)
    manifests = objects(artifacts["gke-test"], "gke-test")
    template = manifests["StatefulSet", "sift-smoke"]["spec"]["template"]
    container, = template["spec"]["containers"]
    service = manifests["Service", "sift-smoke"]["spec"]
    policy = manifests["NetworkPolicy", "sift-smoke"]["spec"]
    require(container["image"] == IMAGE, "smoke workload has the wrong image digest")
    require(container["readinessProbe"]["httpGet"] == {"path": "/readyz", "port": "http"}
            and service["ports"][0]["targetPort"] == "http", "smoke readiness does not match the service")
    require(policy["podSelector"]["matchLabels"] == service["selector"] == template["metadata"]["labels"]
            and policy["policyTypes"] == ["Ingress"]
            and policy["ingress"] == [{
                "from": [{"podSelector": {"matchLabels": {"app.kubernetes.io/name": "sift-smoke-probe"}}}],
                "ports": [{"port": service["ports"][0]["port"], "protocol": "TCP"}],
            }], "smoke network policy must allow only the probe to reach the service port")

    restore = k8s / "overlays/gke-restore-drill"
    kustomization = restore / "kustomization.yaml"
    config = yaml.safe_load(kustomization.read_text())
    config["images"] = [{"name": "sift", "newName": IMAGE_NAME, "digest": DIGEST}]
    kustomization.write_text(yaml.safe_dump(config))
    artifacts["gke-restore-drill"] = run("kubectl", "kustomize", restore)
    artifacts["gke-restore-drill-job"] = run("kubectl", "kustomize", k8s / "gke-test-job/restore-drill")
    drill = objects(artifacts["gke-restore-drill"], "gke-restore-drill")
    drill_job = objects(artifacts["gke-restore-drill-job"], "gke-restore-drill-job")
    drill_template = drill["StatefulSet", "sift-restore-drill"]["spec"]["template"]
    drill_container, = drill_template["spec"]["containers"]
    require(drill_container["image"] == IMAGE
            and drill_template["spec"]["serviceAccountName"] == "sift-restore-drill"
            and drill_template["spec"]["automountServiceAccountToken"] is False
            and drill["ServiceAccount", "sift-restore-drill"]["automountServiceAccountToken"] is False,
            "restore drill workload must use its own service account, the image digest, and no token mount")
    for source_name, manifests in (("gke-restore-drill", drill), ("gke-restore-drill-job", drill_job)):
        for key, item in manifests.items():
            require(item["metadata"].get("namespace") == "sift",
                    f"{source_name}: {key} is not in namespace sift")
            labels = [item["metadata"].get("labels", {}),
                      item.get("spec", {}).get("template", {}).get("metadata", {}).get("labels", {})]
            require(not any("lane" in label for label in labels) and item["metadata"].get("namespace") != "sift-smoke",
                    f"{source_name}: {key} must not carry a lane label or the sift-smoke namespace")

    for variant, filename in (("source", "Dockerfile"), ("release", "Dockerfile.release")):
        output = directory / filename
        run(sift, "dockerfile", "render", "--variant", variant, "--version", version, "--out", output)
        text = output.read_text()
        expected = "\n".join(line for line in (ROOT / filename).read_text().splitlines()
                             if not line.startswith(("# HANDWRITE-BEGIN", "# HANDWRITE-END"))) + "\n"
        require(text == expected.replace("SIFT_VERSION=REPLACE_ME", f"SIFT_VERSION={version}"),
                f"{filename}: CLI render differs from the checked-in artifact")
        require('ENTRYPOINT ["/usr/local/bin/sift"]\nCMD ["serve"]\n' in text,
                f"{filename}: Sift entrypoint or default command changed")
        artifacts[filename] = text

    for name, artifact in artifacts.items():
        require("REPLACE_" not in artifact, f"{name}: unresolved artifact placeholder")
    return artifacts


def main():
    revision = run("git", "rev-parse", "HEAD").strip()
    check_public_tls_rejects_plain_http()
    require(not run("git", "status", "--porcelain", "--untracked-files=no"), "tracked source is dirty")
    target = ROOT / os.environ.get("CARGO_TARGET_DIR", "target")
    env = dict(os.environ, SIFT_SOURCE_REVISION=revision)
    command = ("cargo", "test", "--locked", "--target-dir", str(target), "-p", "sift", "--test", "it",
               "--", "deployment_cli::")
    print(f"Source {revision}\nRunning {' '.join(command)}", flush=True)
    output = run(*command, env=env)
    print(output, end="")
    require(re.search(r"^test result: ok\. [1-9][0-9]* passed;", output, re.MULTILINE),
            "deployment CLI preflight ran no passing tests")
    sift = target / "debug/sift"
    build = json.loads(run(sift, "acceptance-build-info"))
    require(build["git_sha"] == revision, "deployment CLI build does not match HEAD")
    print(f"CLI build target {build['target']}")
    print("Running kubectl kustomize k8s/overlays/prod")
    preflight = run("kubectl", "kustomize", "k8s/overlays/prod")
    require(("Sift", "sift") in objects(preflight, "production overlay preflight"),
            "production overlay did not compose a Sift instance")

    with tempfile.TemporaryDirectory(prefix="sift-deployment-artifacts-") as scratch:
        first = render(Path(scratch) / "first", sift, build["version"])
        second = render(Path(scratch) / "second", sift, build["version"])
        require(first == second, "deployment artifacts differ between clean renders")
        for name, text in first.items():
            print(f"{name} sha256:{hashlib.sha256(text.encode()).hexdigest()}")
    require(run("git", "rev-parse", "HEAD").strip() == revision
            and not run("git", "status", "--porcelain", "--untracked-files=no"), "source changed during the check")
    print("Artifact composition checks passed. Temporary renders removed.")


if __name__ == "__main__":
    main()
