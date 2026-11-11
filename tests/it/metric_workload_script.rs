//! The metric workload script prints evidence and fails only on setup errors or zero work.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn script() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/perf/metric_workload.sh")
}

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "sift-metric-workload-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_workload(tmpdir: &Path, out: &Path, requests: &str, items: &str, queries: &str) -> Output {
    Command::new("bash")
        .arg(script())
        .env("SIFT_BIN", env!("CARGO_BIN_EXE_sift"))
        .env("WORKLOAD_OUT", out)
        .env("WORKLOAD_REQUESTS", requests)
        .env("WORKLOAD_ITEMS", items)
        .env("WORKLOAD_QUERIES", queries)
        .env("TMPDIR", tmpdir)
        .output()
        .expect("run metric_workload.sh with bash")
}

#[test]
fn workload_prints_one_json_line_with_work() {
    let base = unique_dir("json");
    let tmpdir = base.join("tmp");
    fs::create_dir_all(&tmpdir).unwrap();
    let out_file = base.join("out.json");

    let output = run_workload(&tmpdir, &out_file, "2", "10", "3");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "stdout must hold exactly one line: {stdout:?}"
    );

    let json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(json["samples_written"], 20);
    assert!(json["queried_samples"].as_u64().unwrap() > 0);
    assert!(json["samples_per_second"].as_f64().unwrap() > 0.0);
    assert!(json["query_p95_ms"].as_f64().unwrap() >= json["query_p50_ms"].as_f64().unwrap());
    assert!(json["peak_rss_kib"].as_u64().unwrap() > 0);
    assert!(json["cpu_seconds"].as_f64().unwrap() >= 0.0);
    assert_eq!(json["requests"], 2);
    assert_eq!(json["queries"], 3);

    let written = fs::read_to_string(&out_file).expect("WORKLOAD_OUT is written");
    assert_eq!(written.trim_end(), lines[0]);

    fs::remove_dir_all(&base).ok();
}

#[test]
fn workload_exits_one_when_it_writes_nothing() {
    let base = unique_dir("zero");
    let tmpdir = base.join("tmp");
    fs::create_dir_all(&tmpdir).unwrap();
    let out_file = base.join("out.json");

    let output = run_workload(&tmpdir, &out_file, "0", "10", "3");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("workload size must be above zero"),
        "stderr: {stderr}"
    );
    assert!(output.stdout.is_empty(), "stdout must be empty");

    fs::remove_dir_all(&base).ok();
}

#[test]
fn workload_leaves_no_process_or_data_dir() {
    let base = unique_dir("cleanup");
    let tmpdir = base.join("tmp");
    fs::create_dir_all(&tmpdir).unwrap();
    let out_file = base.join("out.json");

    let output = run_workload(&tmpdir, &out_file, "2", "10", "3");

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
fn workload_rejects_non_numeric_size() {
    let base = unique_dir("numeric");
    let tmpdir = base.join("tmp");
    fs::create_dir_all(&tmpdir).unwrap();
    let out_file = base.join("out.json");

    let output = run_workload(&tmpdir, &out_file, "2", "abc", "3");

    assert_eq!(output.status.code(), Some(2));

    fs::remove_dir_all(&base).ok();
}
