//! The Prometheus comparison prints one evidence line, pins the archive digest,
//! reports differences without failing, and leaves nothing behind.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

const PINNED_SHA256: &str = "68727009472a2c02e1ff7f062fd5b3e637285bffc8fc2d340c76fb449d6e0f34";

/// A stub Prometheus: it accepts remote writes, answers readiness and returns one fixed
/// range query. Its data never equals Sift's, so the comparison finds differences.
const PROM_STUB: &str = r##"#!/usr/bin/env python3
import json, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

listen = "127.0.0.1:9090"
for arg in sys.argv[1:]:
    if arg.startswith("--web.listen-address="):
        listen = arg.split("=", 1)[1]
host, port = listen.rsplit(":", 1)

QUERY = {"status": "success", "data": {"resultType": "matrix", "result": [
    {"metric": {"__name__": "x"}, "values": [[1.0, "1"], [2.0, "2"], [3.0, "3"]]}]}}

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def _send(self, status, body=b""):
        self.send_response(status)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def do_GET(self):
        if self.path == "/-/ready":
            self._send(200, b"Prometheus is Ready.\n")
        elif self.path.startswith("/api/v1/query_range"):
            self._send(200, json.dumps(QUERY).encode())
        else:
            self._send(404)

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        self.rfile.read(length)
        self._send(204 if self.path == "/api/v1/write" else 404)

ThreadingHTTPServer((host, int(port)), Handler).serve_forever()
"##;

fn script() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/perf/prometheus_comparison.sh")
}

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "sift-prometheus-comparison-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Builds `prometheus-3.16.0.linux-amd64/prometheus` as a gzip tar and returns its path and sha256.
fn stub_archive(dir: &Path) -> (PathBuf, String) {
    let stage = dir.join("stage");
    let bin_dir = stage.join("prometheus-3.16.0.linux-amd64");
    fs::create_dir_all(&bin_dir).unwrap();
    let bin = bin_dir.join("prometheus");
    fs::write(&bin, PROM_STUB).unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();

    let archive = dir.join("stub.tar.gz");
    let status = Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&stage)
        .arg("prometheus-3.16.0.linux-amd64")
        .status()
        .expect("run tar");
    assert!(status.success(), "tar failed");

    let out = Command::new("sha256sum")
        .arg(&archive)
        .output()
        .expect("run sha256sum");
    let sha = String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    (archive, sha)
}

struct Run<'a> {
    tmpdir: &'a Path,
    url: String,
    override_sha: Option<String>,
    out: &'a Path,
    requests: &'a str,
    items: &'a str,
    queries: &'a str,
}

fn run_comparison(run: &Run) -> Output {
    let mut command = Command::new("bash");
    command
        .arg(script())
        .env("SIFT_REPO_ROOT", env!("CARGO_MANIFEST_DIR"))
        .env("SIFT_BIN", env!("CARGO_BIN_EXE_sift"))
        .env("COMPARISON_OUT", run.out)
        .env("WORKLOAD_REQUESTS", run.requests)
        .env("WORKLOAD_ITEMS", run.items)
        .env("WORKLOAD_QUERIES", run.queries)
        .env("PROMETHEUS_URL", &run.url)
        .env("TMPDIR", run.tmpdir);
    match &run.override_sha {
        Some(sha) => {
            command.env("PROMETHEUS_SHA256_OVERRIDE_FOR_TESTS", sha);
        }
        None => {
            command.env_remove("PROMETHEUS_SHA256_OVERRIDE_FOR_TESTS");
        }
    }
    command.output().expect("run prometheus_comparison.sh with bash")
}

#[test]
fn comparison_prints_both_sides_and_reports_differences() {
    let base = unique_dir("json");
    let tmpdir = base.join("tmp");
    fs::create_dir_all(&tmpdir).unwrap();
    let (archive, sha) = stub_archive(&base);
    let out_file = base.join("out.json");

    let output = run_comparison(&Run {
        tmpdir: &tmpdir,
        url: format!("file://{}", archive.display()),
        override_sha: Some(sha),
        out: &out_file,
        requests: "2",
        items: "10",
        queries: "3",
    });

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "stdout must hold exactly one line: {stdout:?}");

    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["prometheus_version"], "3.16.0");
    assert_eq!(json["workload"]["requests"], 2);
    assert_eq!(json["sift"]["samples_written"], 20);
    assert!(json["sift"]["queried_samples"].as_u64().unwrap() > 0);
    assert_eq!(json["prometheus"]["samples_written"], 20);
    assert_eq!(json["prometheus"]["queried_samples"], 9);
    assert_eq!(json["semantic"]["queries"], 3);
    assert_eq!(json["semantic"]["matching"], 0);
    let differences = json["semantic"]["differences"].as_array().unwrap();
    assert!(!differences.is_empty(), "differences: {differences:?}");
    assert!(differences.len() <= 5);
    assert!(differences.iter().all(|d| d.is_string()));

    let written = fs::read_to_string(&out_file).expect("COMPARISON_OUT is written");
    assert_eq!(written.trim_end(), lines[0]);

    fs::remove_dir_all(&base).ok();
}

#[test]
fn comparison_rejects_an_archive_with_the_wrong_digest() {
    let base = unique_dir("digest");
    let tmpdir = base.join("tmp");
    fs::create_dir_all(&tmpdir).unwrap();
    let (archive, _sha) = stub_archive(&base);
    let out_file = base.join("out.json");

    let output = run_comparison(&Run {
        tmpdir: &tmpdir,
        url: format!("file://{}", archive.display()),
        override_sha: Some("0".repeat(64)),
        out: &out_file,
        requests: "2",
        items: "10",
        queries: "3",
    });

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("prometheus archive digest mismatch"),
        "stderr: {stderr}"
    );
    assert!(output.stdout.is_empty(), "stdout must be empty");

    fs::remove_dir_all(&base).ok();
}

#[test]
fn comparison_ignores_the_override_for_a_non_file_url() {
    let base = unique_dir("https");
    let tmpdir = base.join("tmp");
    fs::create_dir_all(&tmpdir).unwrap();
    let out_file = base.join("out.json");

    let output = run_comparison(&Run {
        tmpdir: &tmpdir,
        url: "https://127.0.0.1:9/never.tar.gz".to_string(),
        override_sha: Some("0".repeat(64)),
        out: &out_file,
        requests: "2",
        items: "10",
        queries: "3",
    });

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("digest mismatch"),
        "the override must not decide for a https URL: {stderr}"
    );

    fs::remove_dir_all(&base).ok();
}

#[test]
fn comparison_rejects_non_numeric_size() {
    let base = unique_dir("numeric");
    let tmpdir = base.join("tmp");
    fs::create_dir_all(&tmpdir).unwrap();
    let out_file = base.join("out.json");

    let output = run_comparison(&Run {
        tmpdir: &tmpdir,
        url: "https://127.0.0.1:9/never.tar.gz".to_string(),
        override_sha: None,
        out: &out_file,
        requests: "abc",
        items: "10",
        queries: "3",
    });

    assert_eq!(output.status.code(), Some(2));

    fs::remove_dir_all(&base).ok();
}

#[test]
fn comparison_leaves_no_temp_dir() {
    let base = unique_dir("cleanup");
    let tmpdir = base.join("tmp");
    fs::create_dir_all(&tmpdir).unwrap();
    let (archive, sha) = stub_archive(&base);
    let out_file = base.join("out.json");

    let output = run_comparison(&Run {
        tmpdir: &tmpdir,
        url: format!("file://{}", archive.display()),
        override_sha: Some(sha),
        out: &out_file,
        requests: "2",
        items: "10",
        queries: "3",
    });

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let leftovers: Vec<_> = fs::read_dir(&tmpdir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(
        leftovers.is_empty(),
        "TMPDIR must be empty after the run: {leftovers:?}"
    );

    fs::remove_dir_all(&base).ok();
}

#[test]
fn document_pin_matches_the_script() {
    let script_text = include_str!("../perf/prometheus_comparison.sh");
    let doc = include_str!("../../docs/operations/prometheus-comparison.md");
    let workflow = include_str!("../../.github/workflows/sift-prometheus-comparison.yml");

    assert!(script_text.contains(PINNED_SHA256), "script pin");
    assert!(script_text.contains("3.16.0"), "script version");
    assert!(doc.contains(PINNED_SHA256), "document pin");
    assert!(doc.contains("3.16.0"), "document version");

    assert!(workflow.contains("workflow_dispatch"));
    assert!(!workflow.contains("pull_request"));
    assert!(!workflow.contains("push:"));
}
