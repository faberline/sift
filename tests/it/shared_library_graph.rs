//! Dependency and explicit-test contracts for Sift and the faberline/core
//! base libraries it composes.

use std::{collections::BTreeSet, path::Path, process::Command};

const SHARED_PACKAGES: &[&str] = &[
    "index-text",
    "metrics-remote-write",
    "service-collector",
    "service-mcp",
    "service-projection",
    "storage-object",
    "storage-segment",
    "transport-otlp",
];

fn metadata() -> serde_json::Value {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("decode cargo metadata")
}

#[test]
fn production_graph_points_from_sift_to_libs_only() {
    let metadata = metadata();
    let packages = metadata["packages"].as_array().unwrap();
    let sift = packages
        .iter()
        .find(|package| package["name"] == "sift")
        .expect("Sift package");
    let production = sift["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|dependency| dependency["kind"].as_str() != Some("dev"))
        .collect::<Vec<_>>();

    for dependency in &production {
        if let Some(path) = dependency["path"].as_str() {
            assert!(
                !path.contains("/apps/"),
                "Sift production dependency points at an app: {path}"
            );
        }
    }
    for expected in SHARED_PACKAGES {
        assert!(
            production
                .iter()
                .any(|dependency| dependency["name"] == *expected),
            "Sift does not compose shared package {expected}"
        );
    }

    // The shared packages are faberline/core git dependencies, so they cannot
    // reach back into apps/sift; their own graph is core's contract.
    for dependency in &production {
        if SHARED_PACKAGES.contains(&dependency["name"].as_str().unwrap_or_default()) {
            let source = dependency["source"].as_str().unwrap_or_default();
            assert!(
                source.starts_with("git+https://github.com/faberline/core"),
                "shared package {} is not a faberline/core git dependency: {source}",
                dependency["name"]
            );
        }
    }
}

#[test]
fn every_integration_test_source_is_compiled() {
    let metadata = metadata();
    // The shared packages' test targets are checked in faberline/core.
    let package = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "sift")
        .expect("Sift package");
    let manifest = Path::new(package["manifest_path"].as_str().unwrap());
    let tests = manifest.parent().unwrap().join("tests");
    let targets = package["targets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "test"))
        })
        .filter_map(|target| target["src_path"].as_str())
        .map(|path| Path::new(path).to_path_buf())
        .collect::<BTreeSet<_>>();
    let rust_sources = |dir: &Path| {
        std::fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("rs"))
            .collect::<Vec<_>>()
    };

    // Process-isolated cases are their own `tests/<name>.rs` binaries.
    for path in rust_sources(&tests) {
        assert!(
            targets.contains(&path),
            "{} is not a Cargo test target",
            path.display()
        );
    }

    // Every other case is a module of the single `it` binary.
    let it = tests.join("it");
    let main = it.join("main.rs");
    assert!(
        targets.contains(&main),
        "{} is not a Cargo test target",
        main.display()
    );
    let main_source = std::fs::read_to_string(&main)
        .unwrap_or_else(|error| panic!("read {}: {error}", main.display()));
    let cases = main_source
        .lines()
        .filter_map(|line| line.strip_prefix("mod ")?.strip_suffix(';'))
        .collect::<BTreeSet<_>>();
    for path in rust_sources(&it) {
        let case = path.file_stem().unwrap().to_str().unwrap();
        assert!(
            case == "main" || cases.contains(case),
            "{} is not a module of {}",
            path.display(),
            main.display()
        );
    }

    // `--test it -- <case>::` is a substring filter, so no case name may end
    // with another case name.
    for case in &cases {
        for other in &cases {
            assert!(
                case == other || !case.ends_with(other),
                "`{other}::` also selects `{case}::`"
            );
        }
    }
}
