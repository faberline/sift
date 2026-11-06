#!/usr/bin/env python3
"""Compare Sift and pinned Prometheus using native Remote Write and PromQL.

Build a clean Sift release with its full Git SHA stamped in the binary.
Run from the checkout with Python 3.11 or newer:

    python3 -B scripts/verify-prometheus-comparison.py --target-dir target

Use the archive cached by scripts/prepare-prometheus-oracle.sh. The benchmark
uses fresh loopback processes and data directories. It makes no cloud calls.
Its default load is 64 series of 1,000 samples in each of four paired trials.
The final JSON records measurements. It gives no production capacity or cost
rating. Use --debug with reduced --series and --trials for development.
"""

import argparse
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import platform
import shutil
import signal
import socket
import subprocess
import sys
import tarfile
import tempfile
import time
from contextlib import closing
from urllib.parse import urlencode


ROOT = Path(__file__).resolve().parents[1]
# Keep this pin in sync with scripts/prepare-prometheus-oracle.sh.
PROMETHEUS_VERSION = "3.5.0"
ARCHIVE_SHA256 = {
    ("darwin", "arm64"): "c17bc2c992f0b5515eebe2b4ccd626214f8e309a0fe6ec40ed13c42ec13d05fb",
    ("darwin", "amd64"): "5856d0c306e7d852d9a79788b20acb630501b3fab120b69f5317d6b0732dea57",
    ("linux", "amd64"): "e811827af26d822afb09a4f28314f61b618b12cff5369835a67f674d8b46f39a",
    ("linux", "arm64"): "173389cc42bf09c4e6e54cb53fa07a5a835d7c261e14775d2183181d6e385d1c",
}
HOST = "127.0.0.1"
PROJECT = "comparison"
ENVIRONMENT = "acceptance"
METRIC = "sift_acceptance_total"
POINTS_PER_SERIES = 1_000
QUERIES_PER_EXPRESSION = 20
MAX_RESPONSE_BYTES = 8 * 1024 * 1024
STOP_SIGNALS = {signal.SIGINT, signal.SIGTERM}
WRITE_HEADERS = {
    "Content-Type": "application/x-protobuf",
    "Content-Encoding": "snappy",
    "X-Prometheus-Remote-Write-Version": "0.1.0",
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def file_hash(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def prometheus_binary(cache, directory):
    system = platform.system().lower()
    arch = {"x86_64": "amd64", "aarch64": "arm64"}.get(platform.machine(), platform.machine())
    expected = ARCHIVE_SHA256.get((system, arch))
    require(expected is not None, f"unsupported Prometheus platform {system}-{arch}")
    name = f"prometheus-{PROMETHEUS_VERSION}.{system}-{arch}"
    archive = cache / f"{name}.tar.gz"
    require(archive.is_file() and not archive.is_symlink(), f"cached archive missing: {archive}")

    binary = directory / "prometheus"
    with archive.open("rb") as stream:
        require(hashlib.file_digest(stream, "sha256").hexdigest() == expected,
                "official Prometheus archive digest mismatch")
        stream.seek(0)
        with tarfile.open(fileobj=stream, mode="r:gz") as package:
            member = package.getmember(f"{name}/prometheus")
            require(member.isfile() and 0 < member.size < 256 * 1024 * 1024,
                    "official archive has an invalid binary")
            with package.extractfile(member) as source, binary.open("xb") as target:
                shutil.copyfileobj(source, target)
    binary.chmod(0o700)
    return binary, expected


def source_revision():
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, timeout=10).strip()


def ports():
    with closing(socket.socket()) as http, closing(socket.socket()) as grpc:
        for listener in (http, grpc):
            listener.bind((HOST, 0))
        return http.getsockname()[1], grpc.getsockname()[1]


def wait_ready(pid, port, path):
    until = time.monotonic() + 20
    while time.monotonic() < until:
        require(os.waitid(os.P_PID, pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is None,
                "service exited before readiness")
        with closing(http.client.HTTPConnection(HOST, port, timeout=0.5)) as connection:
            try:
                connection.request("GET", path)
                reply = connection.getresponse()
                reply.read(4096)
                if reply.status == 200:
                    return
            except OSError:
                pass
        time.sleep(0.05)
    raise ValueError("service readiness timed out")


def stop_process(pid):
    began = time.monotonic()
    previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, STOP_SIGNALS)
    try:
        found, status, usage = os.wait4(pid, os.WNOHANG)
        require(not found, f"service exited before shutdown ({os.waitstatus_to_exitcode(status)})")
        os.kill(pid, signal.SIGTERM)
        while time.monotonic() - began < 30:
            found, status, usage = os.wait4(pid, os.WNOHANG)
            if found:
                require(os.waitstatus_to_exitcode(status) == 0, "service failed to exit cleanly")
                return {
                    "shutdown_seconds": time.monotonic() - began,
                    "cpu_seconds": usage.ru_utime + usage.ru_stime,
                    "peak_rss_bytes": usage.ru_maxrss * (1 if sys.platform == "darwin" else 1024),
                }
            time.sleep(0.02)
        os.kill(pid, signal.SIGKILL)
        os.wait4(pid, 0)
        raise ValueError("service required forced shutdown")
    finally:
        signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)


def seconds(millis):
    return f"{millis // 1000}.{millis % 1000:03d}"


def request(connection, method, path, status, body=None, headers=None):
    connection.request(method, path, body=body, headers=headers or {})
    reply = connection.getresponse()
    raw = reply.read(MAX_RESPONSE_BYTES + 1)
    require(reply.status == status and len(raw) <= MAX_RESPONSE_BYTES,
            f"{method} {path} returned HTTP {reply.status} or an oversized response")
    return raw, reply


def query(connection, api, path, kind, parameters):
    encoded = urlencode({**api["scope"], **parameters})
    raw, _ = request(connection, "GET", f"{api['prefix']}/{path}?{encoded}", 200)
    payload = json.loads(raw)
    require(payload["status"] == "success" and not payload.get("warnings") and not payload.get("infos"),
            f"{path} response was incomplete")
    require(payload["data"]["resultType"] == kind, f"{path} returned a different result type")
    return payload["data"]["result"]


def verify_series(rows, expected_labels, series_count):
    require(len(rows) == series_count, "query returned a different series count")
    names = set()
    for row in rows:
        labels = row["metric"]
        fixture = labels["fixture"]
        require(labels == {**expected_labels, "fixture": fixture}, "query returned different metric labels")
        names.add(fixture)
    require(names == {f"series-{index:02d}" for index in range(series_count)},
            "query returned different series identities")


def verify_sample(sample, millis, value):
    # PromQL timestamps are JSON floats. Tolerate binary rounding below one microsecond.
    require(abs(sample[0] * 1_000 - millis) < 0.001 and float(sample[1]) == value,
            f"query sample differs at timestamp {millis}")


def verify_readback(connection, api, base_ms, end_ms, series_count):
    rows = query(connection, api, "query_range", "matrix",
                 {"query": METRIC, "start": seconds(base_ms), "end": seconds(end_ms), "step": "0.001"})
    verify_series(rows, api["labels"], series_count)
    for row in rows:
        first_ms = base_ms + int(row["metric"]["fixture"].removeprefix("series-"))
        values = row["values"]
        require(len(values) == end_ms - first_ms + 1, "range query returned a different sample count")
        for index, sample in enumerate(values):
            # After a series ends, the instant selector keeps its latest value.
            verify_sample(sample, first_ms + index, min(index, POINTS_PER_SERIES - 1))
    return len(rows) * POINTS_PER_SERIES


def time_queries(connection, api, end_ms, series_count):
    timings = {"latest": [], "aggregate": []}
    for _ in range(QUERIES_PER_EXPRESSION):
        began = time.perf_counter()
        rows = query(connection, api, "query", "vector", {"query": METRIC, "time": seconds(end_ms)})
        timings["latest"].append(time.perf_counter() - began)
        verify_series(rows, api["labels"], series_count)
        for row in rows:
            verify_sample(row["value"], end_ms, POINTS_PER_SERIES - 1)

        began = time.perf_counter()
        rows = query(connection, api, "query", "vector",
                     {"query": f"sum({METRIC})", "time": seconds(end_ms)})
        timings["aggregate"].append(time.perf_counter() - began)
        require(len(rows) == 1 and rows[0]["metric"] == {}, "aggregate returned a different series")
        verify_sample(rows[0]["value"], end_ms, series_count * (POINTS_PER_SERIES - 1))
    return {f"{name}_query_p95_seconds": sorted(values)[math.ceil(len(values) * 0.95) - 1]
            for name, values in timings.items()}


def run_case(case, directory, sift, prometheus, payloads, base_ms):
    directory.mkdir()
    data = directory / "data"
    port, grpc_port = ports()
    is_sift = case == "sift"
    labels = {"__name__": METRIC}
    scope = {"project": PROJECT, "environment": ENVIRONMENT}
    if is_sift:
        command = [sift, "serve", "--role", "all", "--host", HOST, "--port", str(port),
                   "--grpc-port", str(grpc_port), "--data-dir", data, "--log-level", "warn"]
        ready_path = "/readyz"
        api = {"prefix": "/prometheus/api/v1", "scope": scope, "labels": labels}
    else:
        config = directory / "prometheus.yml"
        config.write_text("scrape_configs: []\n", encoding="utf-8")
        command = [prometheus, "--web.enable-remote-write-receiver", "--web.listen-address",
                   f"{HOST}:{port}", "--config.file", config, "--storage.tsdb.path", data,
                   "--log.level=warn"]
        ready_path = "/-/ready"
        api = {"prefix": "/api/v1", "scope": {}, "labels": {**labels, **scope}}

    logfile = directory / "service.log"
    with logfile.open("xb") as log:
        began = time.perf_counter()
        log_actions = [(os.POSIX_SPAWN_DUP2, log.fileno(), fd) for fd in (1, 2)]
        env = {"PATH": os.defpath, "SIFT_AUTH": "off"} if is_sift else {"PATH": os.defpath}
        previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, STOP_SIGNALS)
        pid, failure = None, None
        try:
            pid = os.posix_spawn(command[0], [os.fspath(arg) for arg in command], env,
                                 file_actions=log_actions, setsigmask=previous_mask)
            signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
            wait_ready(pid, port, ready_path)
            startup = time.perf_counter() - began
            with closing(http.client.HTTPConnection(HOST, port, timeout=20)) as connection:
                began = time.perf_counter()
                for payload in payloads:
                    raw, reply = request(connection, "POST", f"{api['prefix']}/write", 204,
                                         body=payload, headers=WRITE_HEADERS)
                    require(not raw, "Remote Write success had a response body")
                    if is_sift:
                        require(reply.getheader("x-prometheus-remote-write-samples-written")
                                == str(POINTS_PER_SERIES), "Sift reported a partial write")
                write_seconds = time.perf_counter() - began
                end_ms = base_ms + len(payloads) + POINTS_PER_SERIES - 2
                read_count = verify_readback(connection, api, base_ms, end_ms, len(payloads))
                query_times = time_queries(connection, api, end_ms, len(payloads))
        except BaseException as error:
            failure = error
            with logfile.open("rb") as saved:
                saved.seek(max(0, logfile.stat().st_size - 2048))
                error.add_note(f"{case} service log tail: {saved.read().decode(errors='replace')}")
            raise
        finally:
            try:
                if pid is not None:
                    lifecycle = stop_process(pid)
            except (OSError, ValueError) as error:
                if failure is None:
                    raise
                failure.add_note(f"{case} shutdown: {error}")
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)

    data_bytes = sum(path.stat().st_size for path in data.rglob("*") if path.is_file())
    require(data_bytes > 0, f"{case} created no on-disk data")
    return {
        "write_requests_204": len(payloads),
        "samples_verified": read_count,
        "write_seconds": write_seconds,
        "write_samples_per_second": read_count / write_seconds,
        "startup_seconds": startup,
        "logical_data_bytes_after_shutdown": data_bytes,
        **query_times,
        **lifecycle,
    }


def interrupt(_number, _frame):
    raise KeyboardInterrupt()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target-dir", type=Path, default=Path(os.getenv("CARGO_TARGET_DIR", "target")))
    parser.add_argument("--oracle-cache", type=Path, default=Path(os.getenv(
        "SIFT_ORACLE_CACHE", Path(tempfile.gettempdir()) / "sift-prometheus-oracle-cache")))
    parser.add_argument("--debug", action="store_true", help="run a debug binary for development evidence")
    parser.add_argument("--series", type=int, default=64, metavar="1..64")
    parser.add_argument("--trials", type=int, default=4, choices=(2, 4, 6, 8))
    args = parser.parse_args()
    require(sys.version_info >= (3, 11), "Python 3.11 or newer is required")
    require(1 <= args.series <= 64, "--series must be from 1 through 64")
    require(args.debug or (args.series == 64 and args.trials >= 4),
            "release comparison requires 64 series and at least four trials")
    profile = "debug" if args.debug else "release"
    sift = (args.target_dir / profile / "sift").resolve()
    revision = source_revision()
    require(len(revision) == 40, "checkout has no full Git SHA")
    if not args.debug:
        require(not subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, timeout=10),
                "release comparison requires a clean checkout")
    build = json.loads(subprocess.check_output([sift, "acceptance-build-info"], timeout=10))
    require(build["git_sha"] == revision, f"Sift binary must be stamped at checkout HEAD {revision}")
    sift_hash = file_hash(sift)
    script_hash = file_hash(Path(__file__))
    signal.signal(signal.SIGTERM, interrupt)

    with tempfile.TemporaryDirectory(prefix="sift-prometheus-comparison-") as temporary:
        directory = Path(temporary)
        prometheus, archive_hash = prometheus_binary(args.oracle_cache, directory)
        base_ms = time.time_ns() // 1_000_000 - 60_000
        payloads = [
            subprocess.check_output([
                sift, "acceptance-payload", "--kind", "prometheus-remote-write-v1",
                "--items", str(POINTS_PER_SERIES), "--project", PROJECT,
                "--event-prefix", f"series-{index:02d}", "--timestamp-unix-nano", str((base_ms + index) * 1_000_000),
            ], timeout=10)
            for index in range(args.series)
        ]
        require(all(payloads), "Sift emitted an empty Remote Write body")
        trials = []
        for index in range(args.trials):
            order = ("sift", "prometheus") if index % 2 == 0 else ("prometheus", "sift")
            trial = {"order": order}
            for case in order:
                trial[case] = run_case(case, directory / f"{index + 1}-{case}", sift,
                                       prometheus, payloads, base_ms)
            trials.append(trial)
    require(not directory.exists(), "benchmark workspace remains after cleanup")
    require(source_revision() == revision and file_hash(sift) == sift_hash
            and file_hash(Path(__file__)) == script_hash, "benchmark inputs changed during the run")
    print(json.dumps({
        "schema": "sift.prometheus-loopback-comparison/v1",
        "result": "passed",
        "environment": "native-loopback",
        "production_accepted": False,
        "cleanup": "verified",
        "sift_build": build,
        "sift_binary_sha256": sift_hash,
        "sift_profile": profile,
        "script_sha256": script_hash,
        "prometheus_version": PROMETHEUS_VERSION,
        "prometheus_archive_sha256": archive_hash,
        "host": {"system": platform.system(), "architecture": platform.machine(),
                 "logical_cpus": os.cpu_count()},
        "workload": {"series": args.series, "samples_per_series": POINTS_PER_SERIES,
                     "samples_per_trial_and_service": args.series * POINTS_PER_SERIES,
                     "first_sample_unix_ms": base_ms, "series_start_offset_ms": 1,
                     "timed_queries_per_expression": QUERIES_PER_EXPRESSION,
                     "payload_sha256": [hashlib.sha256(payload).hexdigest() for payload in payloads]},
        "differences": ["Sift single-process role-all with project and environment scoping and authentication off",
                        "Prometheus Remote Write receiver with scraping disabled",
                        "Independent storage engines with their native durability and file layout",
                        "Cold service starts followed by range readback before timed instant queries"],
        "trials": trials,
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.SubprocessError, KeyboardInterrupt) as error:
        print(f"comparison failed: {error}", file=sys.stderr)
        for note in getattr(error, "__notes__", []):
            print(note, file=sys.stderr)
        raise SystemExit(1)
