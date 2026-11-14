//! scripts/faberline-core-test.sh picks the core checkout of Sift's own crates.

use std::fs;
use std::path::Path;
use std::process::Command;

fn script() -> String {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/faberline-core-test.sh"
    )
    .to_string()
}

#[test]
fn root_is_the_checkout_of_raft_runtime() {
    let output = Command::new("bash")
        .arg(script())
        .arg("--root")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run scripts/faberline-core-test.sh --root");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "--root must exit 0, stderr: {stderr}"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let root = stdout.trim();
    assert!(
        Path::new(root).join("Cargo.toml").is_file(),
        "--root must print a checkout with Cargo.toml, printed: {root:?}"
    );

    // The checkout directory is named by the short commit that cargo uses.
    // It must be a prefix of the full commit that Cargo.lock pins for raft-runtime.
    let lock = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock"))
        .expect("read Cargo.lock");
    let mut lines = lock.lines();
    lines
        .by_ref()
        .find(|line| *line == "name = \"raft-runtime\"")
        .expect("Cargo.lock has a raft-runtime package");
    let source = lines
        .find(|line| line.starts_with("source = "))
        .expect("raft-runtime has a source line");
    let full_commit = source
        .rsplit_once('#')
        .map(|(_, commit)| commit.trim_end_matches('"'))
        .expect("raft-runtime source names a commit");

    let short = Path::new(root)
        .file_name()
        .and_then(|name| name.to_str())
        .expect("checkout directory has a name");
    assert!(
        short.len() >= 7 && full_commit.starts_with(short),
        "checkout {short:?} must be a prefix of the raft-runtime commit {full_commit:?}"
    );
}
