use std::fs;
use std::path::Path;

const DOCS: [&str; 3] = [
    "docs/operations/production-acceptance-runbook.md",
    "docs/production-v1.md",
    "docs/pilot.md",
];

fn read(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[test]
fn operator_docs_have_no_machine_local_paths() {
    for path in DOCS {
        let text = read(path);
        assert!(
            !text.contains("/private/tmp"),
            "{path} still names a /private/tmp work path"
        );
    }
}

#[test]
fn operator_docs_define_the_work_directory_variable() {
    for path in DOCS {
        let text = read(path);
        assert!(
            text.contains(r#"W="${SIFT_WORK_DIR:-$PWD/work}""#),
            "{path} does not define W from SIFT_WORK_DIR"
        );
    }
}

#[test]
fn resources_field_is_documented() {
    for path in ["docs/operations/configuration.md", "docs/single-node-v1.md"] {
        let text = read(path);
        assert!(
            text.contains("spec.resources"),
            "{path} does not mention spec.resources"
        );
    }
    let reference = read("docs/operations/configuration.md");
    assert!(
        reference.contains("The log and metrics agents do not use it."),
        "the reference does not say that the agents ignore spec.resources"
    );
}
