#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "jsonschema==4.26.0",
#     "pydantic==2.13.4",
#     "PyYAML==6.0.3",
# ]
# ///
"""Verify Sift's OpenAPI JSON and three generated HTTP clients on loopback."""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import sys
from tempfile import TemporaryDirectory
import time
import tomllib
from urllib.error import URLError
from urllib.request import ProxyHandler, Request, build_opener

from jsonschema import Draft202012Validator
from jsonschema.exceptions import SchemaError, ValidationError
import yaml


PROJECT = "codegen-smoke"
TRACE_ID = "0af7651916cd43dd8448eb211c80319c"
HTTP = build_opener(ProxyHandler({}))


def capture(command):
    return subprocess.check_output(command, text=True, timeout=30)


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def generate(binary, scratch):
    for language, entrypoint in (("ts", "index.ts"), ("py", "__init__.py"), ("rust", "mod.rs")):
        artifacts = []
        for run in ("first", "second"):
            directory = scratch / run / language
            output = capture([binary, "spec", "gen", "--lang", language, "--out", directory])
            entry = directory / entrypoint
            require(entry.is_file() and output.splitlines()[-1:] == [f"next: {entry}"],
                    f"{language} did not report its generated entry point")
            files = {p.relative_to(directory): p.read_bytes() for p in directory.rglob("*") if p.is_file()}
            require(len(files) > 1 and all(files.values()), f"{language} output is incomplete")
            artifacts.append(files)
        require(artifacts[0] == artifacts[1], f"{language} output changed between cold runs")
        print(f"PASS {language} cold generation ({len(artifacts[0])} files)", flush=True)


@contextmanager
def sift_server(binary, scratch):
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    url = f"http://127.0.0.1:{port}"
    env = {key: value for key, value in os.environ.items() if not key.startswith(("SIFT_", "OTEL_"))}
    log = scratch / "sift.log"
    with log.open("wb") as stream:
        process = subprocess.Popen(
            [binary, "serve", "--host", "127.0.0.1", "--port", str(port), "--grpc-port", "0",
             "--data-dir", scratch / "data", "--grace-secs", "1"],
            env=dict(env, SIFT_AUTH="off"), stdout=stream, stderr=subprocess.STDOUT,
        )
        try:
            deadline = time.monotonic() + 15
            while process.poll() is None and time.monotonic() < deadline:
                try:
                    with HTTP.open(f"{url}/readyz", timeout=1) as response:
                        require(response.status == 200, "unexpected readiness status")
                    break
                except URLError:
                    time.sleep(0.1)
            else:
                raise RuntimeError(f"Sift did not become ready\n{log.read_text()}")
            yield url
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def check_openapi(binary, url):
    terminal = json.loads(capture([binary, "spec", "--format", "openapi-json"]))
    require(terminal.get("next") == "done", "OpenAPI JSON did not report completion")
    spec = terminal["openapi"]
    with HTTP.open(f"{url}/openapi.json", timeout=5) as response:
        require(json.load(response) == spec, "CLI and served OpenAPI documents differ")
    missing = []
    for path, methods in spec["paths"].items():
        for method in {"get", "put", "post", "delete", "options", "head", "patch", "trace"} & methods.keys():
            params = methods.get("parameters", []) + methods[method].get("parameters", [])
            declared = {p.get("name") for p in params if p.get("in") == "path" and p.get("required") is True}
            for name in sorted(set(re.findall(r"\{([^}]+)\}", path)) - declared):
                missing.append(f"{method.upper()} {path} is missing required path parameter {name}")
    for path in ("/api/v1/queries/{query_id}", "/api/v1/traces/{trace_id}"):
        params = spec["paths"][path]["get"].get("parameters", [])
        if not any(p.get("in") == "query" and p.get("name") == "project" and p.get("required") is True
                   for p in params):
            missing.append(f"GET {path} is missing required project query parameter")
    if missing:
        print("\n".join(f"FAIL {message}" for message in missing), file=sys.stderr)
    else:
        print("PASS CLI and served OpenAPI JSON, including required path and project parameters", flush=True)
    return spec, not missing


def check_public_formats(binary, spec):
    passed = True
    for format_name in ("openapi-yaml", "json-schema"):
        try:
            output = capture([binary, "spec", "--format", format_name])
            if format_name == "openapi-yaml":
                require(output.splitlines()[-1:] == ["# next: done"], "OpenAPI YAML did not report completion")
                require(yaml.safe_load(output) == spec, "OpenAPI YAML differs from OpenAPI JSON")
            else:
                check_json_schema(json.loads(output), spec)
        except (RuntimeError, ValueError, KeyError, SchemaError, ValidationError,
                yaml.YAMLError, subprocess.SubprocessError) as error:
            print(f"FAIL {format_name}: {error}", file=sys.stderr)
            passed = False
        else:
            print(f"PASS {format_name} contents and terminal output", flush=True)
    return passed


def check_json_schema(catalog, spec):
    require(catalog.get("next") == "done", "JSON Schema did not report completion")
    require(catalog.get("$schema") == Draft202012Validator.META_SCHEMA["$id"],
            "JSON Schema does not declare Draft 2020-12")
    definitions = catalog["$defs"]
    require(definitions and definitions.keys() == spec["components"]["schemas"].keys(),
            "JSON Schema definition names differ from OpenAPI JSON")
    Draft202012Validator.check_schema(catalog)

    scope = Draft202012Validator({**catalog, "$ref": "#/$defs/InstrumentationScope"})
    scope.validate({"name": "codegen-smoke", "version": None})
    scope.validate({"name": "codegen-smoke", "version": "1.0"})
    require(not scope.is_valid({"name": "codegen-smoke", "version": 1}),
            "JSON Schema accepted a non-string InstrumentationScope version")

    span_scope = Draft202012Validator({
        **catalog, "$ref": "#/$defs/SpanRecordV1/properties/instrumentation_scope",
    })
    span_scope.validate(None)
    span_scope.validate({"name": "codegen-smoke", "version": None})
    require(not span_scope.is_valid({"name": 1}),
            "JSON Schema accepted a non-string name in the referenced InstrumentationScope")


def seed_trace(url):
    now = time.time_ns()
    span = {
        "traceId": TRACE_ID, "spanId": "b7ad6b7169203331", "name": "generated client smoke",
        "startTimeUnixNano": str(now - 1_000_000), "endTimeUnixNano": str(now),
    }
    payload = {"resourceSpans": [{"scopeSpans": [{"spans": [span]}]}]}
    request = Request(f"{url}/v1/traces", json.dumps(payload).encode(), headers={
        "content-type": "application/json", "x-sift-project": PROJECT,
    })
    with HTTP.open(request, timeout=10) as response:
        require(response.status == 200, "Sift did not accept the fixture trace")


def probe(name, command, directory, timeout=30):
    try:
        env = os.environ.copy()
        env.pop("PYTHONOPTIMIZE", None)
        with subprocess.Popen(command, cwd=directory, env=env, start_new_session=True) as process:
            try:
                passed = process.wait(timeout=timeout) == 0
            finally:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGKILL)
    except (OSError, subprocess.TimeoutExpired) as error:
        print(error, file=sys.stderr)
        passed = False
    print(f"{'PASS' if passed else 'FAIL'} {name}", flush=True)
    return passed


def typescript_probe(tsc, directory, url):
    if not probe("TypeScript client type-check", [
        tsc, "--target", "ES2022", "--module", "commonjs", "--strict", "--outDir", "js", "ts/client.ts",
    ], directory):
        return False
    return probe("TypeScript client runtime (4 assertions and trace lookup)", ["node", "-e", """
const assert = require('node:assert/strict');
const {createClient} = require('./js/client.js');
const [baseUrl, project, trace_id] = process.argv.slice(1);
const client = createClient({baseUrl});
client.adminIntegrity({query: {project}}).then(async report => {
    assert.equal(report.version, 1);
    assert.equal(report.project, project);
    assert.equal(report.event_count, 1);
    assert.equal(report.signals.traces.count, 1);
    await client.getApiV1TracesByTraceId({
        path: {trace_id}, query: {project, min_cursor: report.signals.traces.watermark},
    });
}).catch(error => { console.error(error); process.exitCode = 1; });
""", url, PROJECT, TRACE_ID], directory)


def python_probe(python, directory, url):
    return probe("Python client runtime (4 assertions and trace lookup)", [python, "-c", """
import sys
from py import Client
url, project, trace_id = sys.argv[1:]
with Client(url) as client:
    report = client.admin_integrity(project=project)
    assert report.version == 1, report
    assert report.project == project, report
    assert report.event_count == 1, report
    assert report.signals.traces.count == 1, report
    client.get_api_v1_traces_trace_id(
        trace_id=trace_id, project=project, min_cursor=report.signals.traces.watermark,
    )
""", url, PROJECT, TRACE_ID], directory)


def locked_rust_probe_dependencies(directory):
    def external_packages(lockfile):
        return {
            (package["name"], package["version"], package["source"], package.get("checksum"))
            for package in tomllib.loads(lockfile)["package"]
            if "source" in package
        }

    sift_lock = Path("Cargo.lock").read_text()
    scratch_lock = directory / "Cargo.lock"
    scratch_lock.write_text(sift_lock)
    # Replace the Sift root package and keep the locked registry versions.
    subprocess.run(["cargo", "update", "--offline", "--workspace", "--quiet"],
                   cwd=directory, check=True, timeout=30)
    packages = external_packages(scratch_lock.read_text())
    require(packages and packages <= external_packages(sift_lock),
            "Rust probe resolved packages outside Sift's Cargo.lock")


def rust_probe(directory, url):
    (directory / "Cargo.toml").write_text("""[package]
name = "sift_codegen_probe"
version = "0.0.0"
edition = "2021"

[lib]
path = "rust/mod.rs"

[[bin]]
name = "probe"
path = "probe.rs"

[dependencies]
reqwest = { version = "0.12", default-features = false, features = ["blocking", "http2", "json", "rustls-tls-native-roots"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
""")
    (directory / "probe.rs").write_text("""use sift_codegen_probe::client::Client;

fn main() {
    let mut args = std::env::args().skip(1);
    let url = args.next().unwrap();
    let project = args.next().unwrap();
    let trace_id = args.next().unwrap();
    let client = Client::new(url);
    let report = client.admin_integrity(project.clone()).unwrap();
    assert_eq!(report.version, 1);
    assert_eq!(report.project, project);
    assert_eq!(report.event_count, 1);
    assert_eq!(report.signals.traces.count, 1);
    client.get_api_v1_traces_trace_id(trace_id, project, Some(report.signals.traces.watermark)).unwrap();
}
""")
    locked_rust_probe_dependencies(directory)
    return probe("Rust client build and runtime (4 assertions and trace lookup)",
                 ["cargo", "run", "--locked", "--quiet", "--target-dir", "target", "--bin", "probe",
                  "--", url, PROJECT, TRACE_ID],
                 directory, timeout=300)


def main():
    parser = argparse.ArgumentParser(description=__doc__, epilog=(
        "Run uv run --script scripts/verify-cli-codegen.py from the Sift repository root. "
        "The build stamp must match HEAD. Requires TypeScript tsc, Node.js with fetch, and Cargo."
    ))
    default_binary = Path(os.getenv("CARGO_TARGET_DIR", "target")) / "debug" / "sift"
    parser.add_argument("--sift-bin", type=Path, default=os.getenv("SIFT_BIN", default_binary))
    parser.add_argument("--tsc", default="tsc")
    parser.add_argument("--python", default=sys.executable)
    args = parser.parse_args()
    binary = args.sift_bin.resolve(strict=True)
    revision = capture(["git", "rev-parse", "HEAD"]).strip()
    info = json.loads(capture([binary, "acceptance-build-info"]))
    stamp = info["git_sha"]
    require(stamp == revision, f"build stamp {stamp} differs from HEAD {revision}")
    print(f"Sift revision {revision}, binary {binary}", flush=True)

    with TemporaryDirectory(prefix="sift-codegen-") as path:
        scratch = Path(path)
        generate(binary, scratch)
        directory = scratch / "first"
        with sift_server(binary, scratch) as url:
            spec, openapi = check_openapi(binary, url)
            formats = check_public_formats(binary, spec)
            seed_trace(url)
            ts = typescript_probe(args.tsc, directory, url)
            py = python_probe(args.python, directory, url)
            rust = rust_probe(directory, url)
    print("Removed scratch output and stopped the local server", flush=True)
    if not all((openapi, formats, ts, py, rust)):
        return 1
    print("passed")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (RuntimeError, OSError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
