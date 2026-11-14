//! The reproducible image check passes on equal digests, fails on different digests and builds with the pinned arguments.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

const EPOCH: &str = "1700000000";

/// A stub `docker`: it logs every call, prints a builder name for `buildx create`, and writes
/// the digest of build N (from `STUB_DIGEST_N`) to the `--metadata-file` of `buildx build`.
const DOCKER_STUB: &str = r#"#!/usr/bin/env bash
set -euo pipefail
echo "$*" >> "$STUB_LOG"
case "$1 $2" in
  "buildx create")
    echo sift-repro-test
    ;;
  "buildx rm")
    ;;
  "buildx build")
    metadata=""
    prev=""
    for arg in "$@"; do
      if [[ "$prev" == "--metadata-file" ]]; then metadata="$arg"; fi
      prev="$arg"
    done
    n=$(grep -c '^buildx build' "$STUB_LOG")
    var="STUB_DIGEST_$n"
    printf '{"containerimage.digest":"%s"}\n' "${!var}" > "$metadata"
    ;;
  *)
    exit 1
    ;;
esac
"#;

fn script() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/release/verify-reproducible-image.sh"
    )
}

fn digest(fill: char) -> String {
    format!("sha256:{}", fill.to_string().repeat(64))
}

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "sift-reproducible-image-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Creates the stub `docker` in `<base>/bin` and an executable `<base>/fake-sift`.
fn setup(name: &str) -> PathBuf {
    let base = unique_dir(name);
    fs::create_dir_all(base.join("bin")).unwrap();
    write_executable(&base.join("bin/docker"), DOCKER_STUB);
    write_executable(&base.join("fake-sift"), "fake sift binary\n");
    base
}

fn log_lines(base: &Path) -> Vec<String> {
    fs::read_to_string(base.join("log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn run_check(base: &Path, args: &[String], digest1: &str, digest2: &str) -> Output {
    let path = format!(
        "{}:{}",
        base.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new("bash")
        .arg(script())
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("PATH", path)
        .env("SOURCE_DATE_EPOCH", EPOCH)
        .env("STUB_LOG", base.join("log"))
        .env("STUB_DIGEST_1", digest1)
        .env("STUB_DIGEST_2", digest2)
        .output()
        .expect("run verify-reproducible-image.sh with bash")
}

fn binary_args(base: &Path) -> Vec<String> {
    vec![
        "--binary".to_owned(),
        base.join("fake-sift").display().to_string(),
    ]
}

#[test]
fn equal_digests_pass() {
    let base = setup("equal");
    let d1 = digest('a');
    let out = run_check(&base, &binary_args(&base), &d1, &d1);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("reproducible: {d1}\n")
    );
    fs::remove_dir_all(&base).ok();
}

#[test]
fn different_digests_fail() {
    let base = setup("different");
    let d1 = digest('a');
    let d2 = digest('b');
    let out = run_check(&base, &binary_args(&base), &d1, &d2);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("image digests differ"), "stderr: {stderr}");
    assert!(
        stderr.contains(&format!("build 1: {d1}")),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains(&format!("build 2: {d2}")),
        "stderr: {stderr}"
    );
    assert!(out.stdout.is_empty(), "stdout: {:?}", out.stdout);
    fs::remove_dir_all(&base).ok();
}

#[test]
fn builds_use_the_pinned_arguments() {
    let base = setup("pinned");
    let d1 = digest('a');
    let out = run_check(&base, &binary_args(&base), &d1, &d1);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let builds: Vec<String> = log_lines(&base)
        .into_iter()
        .filter(|line| line.starts_with("buildx build"))
        .collect();
    assert_eq!(builds.len(), 2, "log: {builds:?}");
    let required = [
        "--no-cache",
        "--provenance=false",
        "--build-arg SOURCE=staged",
        &format!("--build-arg SOURCE_DATE_EPOCH={EPOCH}"),
        "--platform linux/amd64",
        "Dockerfile.release",
        "type=oci,dest=",
        "rewrite-timestamp=true",
    ];
    for build in &builds {
        for needle in required {
            assert!(build.contains(needle), "missing {needle:?} in: {build}");
        }
    }
    fs::remove_dir_all(&base).ok();
}

#[test]
fn missing_binary_exits_two() {
    let base = setup("missing");
    let missing = base.join("missing").display().to_string();
    let out = run_check(
        &base,
        &["--binary".to_owned(), missing],
        &digest('a'),
        &digest('a'),
    );

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("binary not found"), "stderr: {stderr}");
    assert!(
        !log_lines(&base)
            .iter()
            .any(|line| line.starts_with("buildx build")),
        "a build ran without a binary"
    );
    fs::remove_dir_all(&base).ok();
}
