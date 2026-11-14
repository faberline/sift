//! Keeps the ignored-test inventory honest: every `#[ignore]` test in
//! `tests/it` has a reason, the CI job pins the same count, and the ignored
//! tests page lists each one with its CI job.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Returns `(reason, name)` for every `#[ignore]` attribute in `tests/it`,
/// in file name order. The reason is empty when the attribute has no text.
fn ignored_tests() -> Vec<(String, String)> {
    let mut files: Vec<PathBuf> = fs::read_dir(root().join("tests/it"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    files.sort();

    let mut found = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if !trimmed.starts_with("#[ignore") {
                continue;
            }
            let reason = trimmed
                .split_once("= \"")
                .and_then(|(_, rest)| rest.strip_suffix("\"]"))
                .unwrap_or("")
                .to_string();
            let fn_line = lines[index + 1..]
                .iter()
                .find(|line| line.contains("fn "))
                .unwrap_or_else(|| panic!("no fn after #[ignore] in {}", file.display()));
            let name: String = fn_line
                .split("fn ")
                .nth(1)
                .unwrap()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            found.push((reason, name));
        }
    }
    found
}

#[test]
fn every_ignored_test_has_a_reason() {
    let tests = ignored_tests();
    assert!(!tests.is_empty(), "no #[ignore] tests found in tests/it");
    for (reason, name) in &tests {
        assert!(
            reason.chars().count() >= 10,
            "#[ignore] on `{name}` needs a reason of at least 10 characters, got {reason:?}"
        );
    }
}

#[test]
fn ignored_count_matches_the_ci_pin() {
    let ci = fs::read_to_string(root().join(".github/workflows/ci.yml")).unwrap();
    let lines: Vec<&str> = ci.lines().collect();
    let step = lines
        .iter()
        .position(|line| line.contains("ignored inventory"))
        .expect("ci.yml has no `ignored inventory` step");
    let pin_line = lines[step..]
        .iter()
        .find(|line| line.contains("-eq "))
        .expect("the `ignored inventory` step has no `-eq` pin");
    let pinned: usize = pin_line
        .split("-eq ")
        .nth(1)
        .unwrap()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .expect("the pin is a number");
    assert_eq!(
        pinned,
        ignored_tests().len(),
        "ci.yml pins {pinned} ignored tests; tests/it has a different count"
    );
}

#[test]
fn ignored_tests_page_lists_every_test() {
    let page = fs::read_to_string(root().join("docs/operations/ignored-tests.md")).unwrap();
    let tests = ignored_tests();
    for (_, name) in &tests {
        let needle = format!("`{name}`");
        let rows: Vec<&str> = page
            .lines()
            .filter(|line| line.starts_with("| ") && line.contains(&needle))
            .collect();
        assert_eq!(
            rows.len(),
            1,
            "`{name}` must have exactly one row in ignored-tests.md"
        );
        assert!(
            rows[0].contains("test-ignored"),
            "the row for `{name}` must name its CI job"
        );
    }
    let table_rows = page.lines().filter(|line| line.starts_with("| `")).count();
    assert_eq!(
        table_rows,
        tests.len(),
        "ignored-tests.md has a row for a test that is not ignored"
    );
}
