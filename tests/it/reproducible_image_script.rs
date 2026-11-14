//! The reproducible image check passes on equal digests, fails on different digests and builds with the pinned arguments.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

const EPOCH: &str = "1700000000";

/// A stub `docker`: it logs every call, fakes the builder and writes the digest of the
/// n-th `buildx build` (n counts the build lines in the log) into `--metadata-file`.
const DOCKER_STUB: &str = r#"#!/usr/bin/env bash
echo "$*" >> "$STUB_LOG"
case "$1 $2" in
  "buildx create")
    echo sift-repro-test
    exit 0
    ;;
  "buildx rm")
    exit 0
    ;;
  "buildx build")
    meta=""
    prev=""
    for arg in "$@"; do
      if [ "$prev" = "--metadata-file" ]; then meta="$arg"; fi
      prev="$arg"
    done
    n=$(grep -c '^buildx build' "$STUB_LOG")
    var="STUB_DIGEST_$n"
    printf '{"containerimage.digest":"%s"}\n' "${!var}" > "$meta"
    exit 0
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

/// A temporary directory with a `bin/docker` stub and a fake binary; removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "sift-reproducible-image-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(dir.join("bin")).unwrap();

        let docker = dir.join("bin/docker");
        fs::write(&docker, DOCKER_STUB).unwrap();
        fs::set_permissions(&docker, fs::Permissions::from_mode(0o755)).unwrap();

        let binary = dir.join("fake-sift");
        fs::write(&binary, "not a real binary\n").unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();

        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn binary(&self) -> PathBuf {
        self.0.join("fake-sift")
    }

    fn log(&self) -> String {
        fs::read_to_string(self.0.join("log")).unwrap_or_default()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_check(scratch: &Scratch, binary: &Path, digest1: &str, digest2: &str) -> Output {
    let path = format!(
        "{}:{}",
        scratch.path().join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new("bash")
        .arg(script())
        .arg("--binary")
        .arg(binary)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("PATH", path)
        .env("SOURCE_DATE_EPOCH", EPOCH)
        .env("STUB_LOG", scratch.path().join("log"))
        .env("STUB_DIGEST_1", digest1)
        .env("STUB_DIGEST_2", digest2)
        .output()
        .expect("run verify-reproducible-image.sh")
}

#[test]
fn equal_digests_pass() {
    let scratch = Scratch::new("equal");
    let d1 = digest('a');
    let out = run_check(&scratch, &scratch.binary(), &d1, &d1);

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("reproducible: {d1}\n")
    );
}

#[test]
fn different_digests_fail() {
    let scratch = Scratch::new("different");
    let d1 = digest('a');
    let d2 = digest('b');
    let out = run_check(&scratch, &scratch.binary(), &d1, &d2);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("image digests differ"), "stderr: {stderr}");
    assert!(
        stderr.contains(&format!("build 1: {d1}")),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains(&format!("build 2: {d2}")),
        "stderr: {stderr}"
    );
    assert!(out.stdout.is_empty(), "stdout must stay empty on failure");
}

#[test]
fn builds_use_the_pinned_arguments() {
    let scratch = Scratch::new("pinned");
    let d1 = digest('a');
    let out = run_check(&scratch, &scratch.binary(), &d1, &d1);
    assert_eq!(out.status.code(), Some(0));

    let log = scratch.log();
    let builds: Vec<&str> = log
        .lines()
        .filter(|line| line.starts_with("buildx build"))
        .collect();
    assert_eq!(builds.len(), 2, "log: {log}");

    let required = [
        "--no-cache",
        "--provenance=false",
        "--build-arg SOURCE=staged",
        "--build-arg SOURCE_DATE_EPOCH=1700000000",
        "--platform linux/amd64",
        "Dockerfile.release",
        "type=oci,dest=",
        "rewrite-timestamp=true",
    ];
    for build in &builds {
        for arg in required {
            assert!(build.contains(arg), "build line misses `{arg}`: {build}");
        }
    }
}

#[test]
fn missing_binary_exits_two() {
    let scratch = Scratch::new("missing");
    let missing = scratch.path().join("missing");
    let out = run_check(&scratch, &missing, &digest('a'), &digest('a'));

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("binary not found"), "stderr: {stderr}");
    assert!(
        !scratch
            .log()
            .lines()
            .any(|line| line.starts_with("buildx build")),
        "no build may start without a binary"
    );
}
