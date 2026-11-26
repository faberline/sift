//! Pins the schema of the `Sift` CRD with a committed golden JSON file.
//!
//! The golden file is created once by the capture run:
//! `SIFT_CAPTURE_CRD_SCHEMA=1 cargo test -p sift --test it crd_schema`.

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/crd/sift-crd.json"
);
const CRD_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/k8s/crd/sift.yaml");

fn crd_value(yaml: &str) -> serde_json::Value {
    serde_yaml::from_str::<serde_json::Value>(yaml).unwrap()
}

fn pretty(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap() + "\n"
}

fn golden(actual: &str) -> String {
    if std::env::var_os("SIFT_CAPTURE_CRD_SCHEMA").is_some() {
        std::fs::create_dir_all(std::path::Path::new(GOLDEN).parent().unwrap()).unwrap();
        std::fs::write(GOLDEN, actual).unwrap();
    }
    std::fs::read_to_string(GOLDEN).unwrap()
}

fn file_schema() -> String {
    let text = std::fs::read_to_string(CRD_FILE).unwrap();
    pretty(&crd_value(&text))
}

#[test]
fn crd_schema_file_does_not_change() {
    let actual = file_schema();
    assert_eq!(
        actual,
        golden(&actual),
        "the Sift CRD in k8s/crd/sift.yaml changed; update {GOLDEN} on purpose in the same pull request"
    );
}

#[test]
fn crd_schema_shipped_by_the_binary_does_not_change() {
    let shipped = pretty(&crd_value(&sift::operations::crd_yaml()));
    assert_eq!(
        shipped,
        golden(&shipped),
        "the Sift CRD shipped by the binary changed; update {GOLDEN} on purpose in the same pull request"
    );
}

#[test]
fn crd_schema_golden_names_the_surface() {
    let golden_value: serde_json::Value = serde_json::from_str(&golden(&file_schema())).unwrap();
    assert_eq!(golden_value["spec"]["group"], "sift.axiom.dev");
    assert_eq!(golden_value["spec"]["names"]["kind"], "Sift");
    assert_eq!(golden_value["spec"]["versions"][0]["name"], "v1alpha1");
    let required = &golden_value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]
        ["spec"]["required"];
    assert!(
        required
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "image"),
        "the spec schema must require `image`: {required}"
    );
}

#[test]
fn crd_schema_check_detects_a_change() {
    let expected = golden(&file_schema());
    let mut changed: serde_json::Value = serde_json::from_str(&expected).unwrap();
    assert_eq!(changed["spec"]["versions"][0]["name"], "v1alpha1");
    changed["spec"]["versions"][0]["name"] = serde_json::Value::String("v1".into());
    assert_ne!(
        pretty(&changed),
        expected,
        "a changed version name must differ from the golden file"
    );
}

#[test]
fn crd_schema_lists_topology() {
    let value = crd_value(&std::fs::read_to_string(CRD_FILE).unwrap());
    let topology = &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"]
        ["properties"]["topology"];
    assert_eq!(topology["enum"], serde_json::json!(["ha", "singleNode"]));
    assert_eq!(topology["default"], "ha");
}

#[test]
fn crd_schema_lists_metrics_namespaces() {
    let value = crd_value(&std::fs::read_to_string(CRD_FILE).unwrap());
    let namespaces = &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]
        ["spec"]["properties"]["metrics"]["properties"]["namespaces"];
    assert_eq!(namespaces["type"], "array");
    assert_eq!(namespaces["maxItems"], 32);
    assert_eq!(
        namespaces["items"]["pattern"],
        "^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"
    );
}

#[test]
fn crd_schema_lists_retention_days() {
    let value = crd_value(&std::fs::read_to_string(CRD_FILE).unwrap());
    let retention = &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]
        ["spec"]["properties"]["retentionDays"];
    assert_eq!(retention["type"], "integer");
    assert_eq!(retention["minimum"], 1);
    assert_eq!(retention["maximum"], 180);
    assert_eq!(retention["default"], 180);
}

#[test]
fn crd_schema_lists_log_pod_label() {
    let value = crd_value(&std::fs::read_to_string(CRD_FILE).unwrap());
    let pod_label = &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]
        ["spec"]["properties"]["logs"]["properties"]["podLabel"];
    assert_eq!(pod_label["type"], "string");
    assert_eq!(pod_label["maxLength"], 320);
    assert!(
        pod_label["pattern"]
            .as_str()
            .is_some_and(|pattern| !pattern.is_empty()),
        "podLabel needs a pattern: {pod_label}"
    );
}

#[test]
fn crd_schema_lists_logs_agent() {
    let value = crd_value(&std::fs::read_to_string(CRD_FILE).unwrap());
    let agent = &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"]
        ["properties"]["logs"]["properties"]["agent"];
    assert_eq!(agent["type"], "string");
    assert_eq!(agent["enum"], serde_json::json!(["enabled", "disabled"]));
    assert_eq!(agent["default"], "enabled");
}

#[test]
fn crd_schema_lists_log_namespaces() {
    let value = crd_value(&std::fs::read_to_string(CRD_FILE).unwrap());
    let namespaces = &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]
        ["spec"]["properties"]["logs"]["properties"]["namespaces"];
    assert_eq!(namespaces["type"], "array");
    assert_eq!(namespaces["maxItems"], 32);
    assert_eq!(
        namespaces["items"]["pattern"],
        "^[a-z0-9]([-a-z0-9]*[a-z0-9])?$"
    );
}

#[test]
fn crd_schema_lists_tolerations_and_storage_class() {
    let value = crd_value(&std::fs::read_to_string(CRD_FILE).unwrap());
    let spec = &value["spec"]["versions"][0]["schema"]["openAPIV3Schema"]["properties"]["spec"]
        ["properties"];

    let tolerations = &spec["placement"]["properties"]["tolerations"];
    assert_eq!(tolerations["type"], "array");
    let item = &tolerations["items"];
    let mut keys: Vec<&str> = item["properties"]
        .as_object()
        .expect("toleration properties")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["effect", "key", "operator", "tolerationSeconds", "value"]
    );
    assert!(item.get("x-kubernetes-preserve-unknown-fields").is_none());
    assert_eq!(
        item["properties"]["effect"]["enum"],
        serde_json::json!(["NoSchedule", "PreferNoSchedule", "NoExecute"])
    );

    assert_eq!(
        spec["storage"]["properties"]["storageClass"]["type"],
        "string"
    );
}
