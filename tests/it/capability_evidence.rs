// Evidence audit for the README capabilities (issue #7): every capability
// names a real owner source and a real executable gate.

use std::path::Path;

const README: &str = include_str!("../../README.md");

const CAPABILITY_IDS: [&str; 8] = [
    "agent-and-cli-access",
    "archive-and-restore",
    "durable-local-data",
    "kubernetes-operation",
    "replicated-availability",
    "signal-ingest",
    "structured-log-collection",
    "unified-investigation",
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[derive(Debug, PartialEq)]
struct Capability {
    id: String,
    sources: Vec<String>,
    gates: Vec<String>,
}

fn after_capabilities(readme: &str) -> &str {
    let start = readme
        .find("## Capabilities")
        .expect("## Capabilities section");
    &readme[start..]
}

fn first_backticked(line: &str) -> String {
    let start = line.find('`').expect("opening backtick") + 1;
    let len = line[start..].find('`').expect("closing backtick");
    line[start..start + len].to_string()
}

// The capability IDs in the table under `### Capability index`.
fn parse_index(readme: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut in_index = false;
    for line in after_capabilities(readme).lines() {
        if line.starts_with("### Capability index") {
            in_index = true;
        } else if line.starts_with("### ") {
            in_index = false;
        } else if in_index && line.starts_with('|') {
            let id = line
                .split('|')
                .nth(2)
                .unwrap_or("")
                .trim()
                .trim_matches('`');
            if !id.is_empty() && id != "ID" && !id.starts_with('-') {
                ids.push(id.to_string());
            }
        }
    }
    ids
}

// The `- ID:` blocks with their `Sources` and `Gate` lines.
fn parse_sections(readme: &str) -> Vec<Capability> {
    let mut capabilities = Vec::new();
    let mut current: Option<Capability> = None;
    for line in after_capabilities(readme).lines() {
        if line.starts_with("### ") {
            capabilities.extend(current.take());
        } else if line.starts_with("- ID: `") {
            capabilities.extend(current.take());
            current = Some(Capability {
                id: first_backticked(line),
                sources: Vec::new(),
                gates: Vec::new(),
            });
        } else if let Some(capability) = current.as_mut() {
            if line.starts_with("  - `") {
                capability.sources.push(first_backticked(line));
            } else if line.starts_with("- Gate: `") {
                capability.gates.push(first_backticked(line));
            }
        }
    }
    capabilities.extend(current);
    capabilities
}

fn check_source(source: &str, cargo_toml: &str) -> Result<(), String> {
    if source == "sift" || source.starts_with("external:") {
        return Ok(());
    }
    if let Some(name) = source.strip_prefix("core/") {
        let key = format!("{name} = ");
        return if cargo_toml.lines().any(|line| line.starts_with(&key)) {
            Ok(())
        } else {
            Err(format!("source {source} is not a Cargo dependency"))
        };
    }
    Err(format!("unknown source form {source}"))
}

// The file must exist and contain a test attribute.
fn require_test_file(path: &Path, gate: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|_| format!("gate {gate}: missing {}", path.display()))?;
    let is_test = |line: &str| {
        let line = line.trim();
        line == "#[test]" || line == "#[tokio::test]" || line.starts_with("#[tokio::test(")
    };
    if text.lines().any(is_test) {
        Ok(())
    } else {
        Err(format!("gate {gate}: {} has no test", path.display()))
    }
}

fn check_gate(gate: &str, root: &Path) -> Result<(), String> {
    if let Some(rest) = gate.strip_prefix("bash ./test.sh --test ") {
        if let Some(case) = rest.strip_prefix("it -- ") {
            let case = case
                .strip_suffix("::")
                .ok_or_else(|| format!("unknown gate form {gate}"))?;
            require_test_file(&root.join("tests/it").join(format!("{case}.rs")), gate)?;
            let main = std::fs::read_to_string(root.join("tests/it/main.rs"))
                .map_err(|error| format!("gate {gate}: {error}"))?;
            let module = format!("mod {case};");
            return if main.lines().any(|line| line.trim() == module.as_str()) {
                Ok(())
            } else {
                Err(format!("gate {gate}: tests/it/main.rs has no `{module}`"))
            };
        }
        if !rest.contains(' ') {
            return require_test_file(&root.join("tests").join(format!("{rest}.rs")), gate);
        }
        return Err(format!("unknown gate form {gate}"));
    }
    if let Some(path) = gate.strip_prefix("bash ") {
        if path.starts_with("tests/") {
            return if root.join(path).is_file() {
                Ok(())
            } else {
                Err(format!("gate {gate}: missing {path}"))
            };
        }
    }
    Err(format!("unknown gate form {gate}"))
}

#[test]
fn index_and_sections_list_the_same_capabilities() {
    let mut from_index = parse_index(README);
    from_index.sort();
    let mut from_sections: Vec<String> = parse_sections(README)
        .into_iter()
        .map(|capability| capability.id)
        .collect();
    from_sections.sort();

    assert_eq!(
        from_index, from_sections,
        "the index and the sections differ"
    );
    let mut expected: Vec<&str> = CAPABILITY_IDS.to_vec();
    expected.sort();
    assert_eq!(
        from_index, expected,
        "unexpected capability IDs in the README index"
    );
}

#[test]
fn every_capability_has_an_owner_and_a_gate() {
    for capability in parse_sections(README) {
        assert!(
            capability
                .sources
                .iter()
                .any(|source| source.as_str() == "sift"),
            "capability {} has no `sift` owner source",
            capability.id
        );
        assert!(
            !capability.gates.is_empty(),
            "capability {} has no gate",
            capability.id
        );
    }
}

#[test]
fn every_core_source_is_a_cargo_dependency() {
    let cargo_toml = std::fs::read_to_string(root().join("Cargo.toml")).expect("Cargo.toml");
    let mut core_sources = 0;
    for capability in parse_sections(README) {
        for source in &capability.sources {
            if source.starts_with("core/") {
                core_sources += 1;
            }
            if let Err(error) = check_source(source, &cargo_toml) {
                panic!("capability {}: {error}", capability.id);
            }
        }
    }
    assert!(
        core_sources >= 10,
        "found only {core_sources} core/ sources"
    );
}

#[test]
fn every_gate_points_at_a_real_test() {
    let mut gates = 0;
    for capability in parse_sections(README) {
        for gate in &capability.gates {
            gates += 1;
            if let Err(error) = check_gate(gate, root()) {
                panic!("capability {}: {error}", capability.id);
            }
        }
    }
    assert!(gates >= 25, "found only {gates} gates");
}

#[test]
fn gate_check_rejects_a_missing_case() {
    let error = check_gate("bash ./test.sh --test it -- no_such_case::", root())
        .expect_err("a missing case must fail");
    assert!(
        error.contains("no_such_case"),
        "error does not name the case: {error}"
    );
    assert!(check_gate("bash ./test.sh --test no_such_binary", root()).is_err());
    assert!(check_gate("echo hi", root()).is_err());
}

#[test]
fn source_check_rejects_an_unknown_core_library() {
    let cargo_toml = "index-text = { git = \"x\" }\n";
    assert!(check_source("core/no-such-library", cargo_toml).is_err());
    assert!(check_source("core/index-text", cargo_toml).is_ok());
}

#[test]
fn parser_reads_a_small_readme() {
    let readme = "# Doc\n\n## Capabilities\n\n- ID: `a`\n- Promise: x\n- Sources:\n  - `sift` owns x.\n- Gate: `bash tests/x.sh`\n";
    let capabilities = parse_sections(readme);
    assert_eq!(
        capabilities,
        vec![Capability {
            id: "a".to_string(),
            sources: vec!["sift".to_string()],
            gates: vec!["bash tests/x.sh".to_string()],
        }]
    );
}
