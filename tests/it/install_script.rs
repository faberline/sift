//! install.sh fails closed when the .sha256 asset of the release archive is missing.
//! The tests use a fake curl on PATH. They never reach the network.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy)]
enum Sidecar {
    Good,
    Missing,
    Wrong,
}

fn script() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/install.sh")
}

fn target() -> String {
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "aarch64"
    };
    let os = if cfg!(target_os = "macos") {
        "apple-darwin"
    } else {
        "unknown-linux-gnu"
    };
    format!("{arch}-{os}")
}

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "sift-install-script-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

const FAKE_CURL: &str = r#"#!/bin/sh
out=""
url=""
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out="$2"; shift 2 ;;
    -*) shift ;;
    *) url="$1"; shift ;;
  esac
done
case "$url" in
  *.sha256)
    if [ "${FAKE_SHA256:-}" = missing ]; then exit 22; fi
    cp "$FAKE_DIR/asset.tar.gz.sha256" "$out" ;;
  *)
    cp "$FAKE_DIR/asset.tar.gz" "$out" ;;
esac
"#;

/// Builds the archive, the sidecar and the fake curl. Returns (work dir, fake bin dir).
fn fixture(sidecar: Sidecar) -> (PathBuf, PathBuf) {
    let work = unique_dir("work");
    let fakebin = unique_dir("bin");
    let target = target();

    let pkg = work.join("files").join(format!("sift-{target}"));
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("sift"), "#!/bin/sh\necho fake\n").unwrap();
    let tar = Command::new("tar")
        .arg("-C")
        .arg(work.join("files"))
        .arg("-czf")
        .arg(work.join("asset.tar.gz"))
        .arg(format!("sift-{target}"))
        .status()
        .expect("run tar");
    assert!(tar.success(), "tar must pack the fake archive");

    match sidecar {
        Sidecar::Good => {
            let sum = Command::new("sh")
                .arg("-c")
                .arg("sha256sum asset.tar.gz 2>/dev/null || shasum -a 256 asset.tar.gz")
                .current_dir(&work)
                .output()
                .expect("compute sha256 of the fake archive");
            let hex = String::from_utf8_lossy(&sum.stdout)
                .split_whitespace()
                .next()
                .expect("a sha256 hex value")
                .to_string();
            fs::write(
                work.join("asset.tar.gz.sha256"),
                format!("{hex}  asset.tar.gz\n"),
            )
            .unwrap();
        }
        Sidecar::Wrong => {
            fs::write(
                work.join("asset.tar.gz.sha256"),
                format!("{}  asset.tar.gz\n", "0".repeat(64)),
            )
            .unwrap();
        }
        Sidecar::Missing => {}
    }

    let curl = fakebin.join("curl");
    fs::write(&curl, FAKE_CURL).unwrap();
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).unwrap();

    (work, fakebin)
}

fn run(work: &Path, fakebin: &Path, skip: bool, sidecar_missing: bool) -> (Output, PathBuf) {
    let install = work.join("install");
    let path = format!(
        "{}:{}",
        fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut cmd = Command::new("sh");
    cmd.arg(script())
        .env("PATH", path)
        .env("HOME", work)
        .env("SIFT_VERSION", "sift@0.0.0")
        .env("SIFT_INSTALL", &install)
        .env("FAKE_DIR", work)
        .env_remove("SIFT_INSTALL_SKIP_VERIFY")
        .env_remove("SIFT_REPO")
        .env_remove("FAKE_SHA256");
    if sidecar_missing {
        cmd.env("FAKE_SHA256", "missing");
    }
    if skip {
        cmd.env("SIFT_INSTALL_SKIP_VERIFY", "1");
    }
    let output = cmd.output().expect("run install.sh with sh");
    (output, install)
}

#[test]
fn missing_sha256_asset_fails_closed() {
    let (work, fakebin) = fixture(Sidecar::Missing);
    let (output, install) = run(&work, &fakebin, false, true);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(4), "stderr: {stderr}");
    assert!(
        stderr.contains("checksum asset missing"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("SIFT_INSTALL_SKIP_VERIFY"),
        "stderr: {stderr}"
    );
    assert!(
        !install.join("sift").exists(),
        "no binary may be installed without a checksum"
    );
}

#[test]
fn skip_verify_installs_and_says_so() {
    let (work, fakebin) = fixture(Sidecar::Missing);
    let (output, install) = run(&work, &fakebin, true, true);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert!(
        stderr.contains("checksum verification skipped"),
        "stderr: {stderr}"
    );
    assert!(install.join("sift").exists(), "stderr: {stderr}");
}

#[test]
fn matching_sha256_installs() {
    let (work, fakebin) = fixture(Sidecar::Good);
    let (output, install) = run(&work, &fakebin, false, false);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert!(stderr.contains("sha256 verified"), "stderr: {stderr}");
    assert!(!stderr.contains("skipped"), "stderr: {stderr}");
    assert!(install.join("sift").exists(), "stderr: {stderr}");
}

#[test]
fn wrong_sha256_is_refused() {
    let (work, fakebin) = fixture(Sidecar::Wrong);
    let (output, install) = run(&work, &fakebin, false, false);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("sha256 mismatch"), "stderr: {stderr}");
    assert!(!install.join("sift").exists(), "stderr: {stderr}");
}
