//! The Kubernetes install is split by apply identity (issue #373).
//!
//! These checks read the checked-in manifests and kustomizations. They run no kubectl.

use std::path::Path;

use serde::Deserialize;
use serde_yaml::Value;
use sift::operations::operator_yaml_with_image;

fn exists(relative: &str) -> bool {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(relative)
        .exists()
}

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn parse_yaml_documents(body: &str) -> Vec<Value> {
    serde_yaml::Deserializer::from_str(body)
        .map(|document| Value::deserialize(document).expect("parse Kubernetes YAML document"))
        .collect()
}

fn documents(dir: &str, file: &str) -> Vec<Value> {
    parse_yaml_documents(&read(&format!("{dir}/{file}")))
        .into_iter()
        .filter(|document| !document.is_null())
        .collect()
}

fn kinds(dir: &str, file: &str) -> Vec<String> {
    documents(dir, file)
        .iter()
        .map(|document| document["kind"].as_str().expect("kind").to_string())
        .collect()
}

fn resources(dir: &str) -> Vec<String> {
    let kustomization: Value = serde_yaml::from_str(&read(&format!("{dir}/kustomization.yaml")))
        .expect("parse kustomization");
    kustomization["resources"]
        .as_sequence()
        .expect("resources list")
        .iter()
        .map(|item| item.as_str().expect("resource path").to_string())
        .collect()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn cluster_part_holds_only_cluster_scoped_objects() {
    assert_eq!(
        resources("k8s/operator/cluster"),
        strings(&["../../crd", "clusterrole.yaml", "clusterrolebinding.yaml"])
    );
    assert_eq!(
        kinds("k8s/operator/cluster", "clusterrole.yaml"),
        strings(&["ClusterRole"])
    );
    assert_eq!(
        kinds("k8s/operator/cluster", "clusterrolebinding.yaml"),
        strings(&["ClusterRoleBinding"])
    );
    assert_eq!(
        kinds("k8s/crd", "sift.yaml"),
        strings(&["CustomResourceDefinition"])
    );
}

#[test]
fn namespace_part_holds_only_the_namespace() {
    assert_eq!(
        resources("k8s/operator/namespace"),
        strings(&["namespace.yaml"])
    );
    assert_eq!(
        kinds("k8s/operator/namespace", "namespace.yaml"),
        strings(&["Namespace"])
    );
}

#[test]
fn namespaced_part_has_no_cluster_scoped_rbac_or_secret() {
    assert_eq!(
        resources("k8s/operator/namespaced"),
        strings(&["serviceaccount.yaml", "deployment.yaml"])
    );
    assert_eq!(
        kinds("k8s/operator/namespaced", "serviceaccount.yaml"),
        strings(&["ServiceAccount"])
    );
    assert_eq!(
        kinds("k8s/operator/namespaced", "deployment.yaml"),
        strings(&["Deployment"])
    );
    for file in ["serviceaccount.yaml", "deployment.yaml"] {
        for document in documents("k8s/operator/namespaced", file) {
            assert_eq!(
                document["metadata"]["namespace"].as_str(),
                Some("sift-system"),
                "{file} must set the sift-system namespace"
            );
        }
    }
}

#[test]
fn operator_and_base_compose_all_parts_once() {
    assert_eq!(
        resources("k8s/operator"),
        strings(&["cluster", "namespace", "namespaced"])
    );
    assert_eq!(resources("k8s/base"), strings(&["../operator"]));
    assert!(
        !exists("k8s/operator/operator.yaml"),
        "the one-file operator must be gone"
    );
}

#[test]
fn instance_overlays_hold_only_the_sift_object() {
    for overlay in ["dev", "staging", "template"] {
        let dir = format!("k8s/overlays/{overlay}");
        assert_eq!(resources(&dir), strings(&["sift.yaml"]), "{overlay}");
        assert_eq!(kinds(&dir, "sift.yaml"), strings(&["Sift"]), "{overlay}");
    }
    let prod = "k8s/overlays/prod";
    assert_eq!(resources(prod), strings(&["sift.yaml", "public-tls.yaml"]));
    assert_eq!(kinds(prod, "sift.yaml"), strings(&["Sift"]));
}

#[test]
fn single_node_overlay_does_not_list_the_crd_twice() {
    let list = resources("k8s/overlays/gke-single-node");
    assert!(list.contains(&"../../operator".to_string()));
    assert!(list.contains(&"sift.yaml".to_string()));
    assert!(!list.contains(&"../../crd".to_string()));
}

#[test]
fn rendered_operator_keeps_all_five_objects() {
    let rendered =
        operator_yaml_with_image("sift-system", "example.invalid/sift:1").expect("render operator");
    let found = parse_yaml_documents(&rendered);
    let found_kinds: Vec<&str> = found
        .iter()
        .map(|document| document["kind"].as_str().expect("kind"))
        .collect();
    assert_eq!(
        found_kinds,
        [
            "Namespace",
            "ServiceAccount",
            "ClusterRole",
            "ClusterRoleBinding",
            "Deployment"
        ]
    );
    let deployment = found
        .iter()
        .find(|document| document["kind"] == "Deployment")
        .expect("Deployment in the render");
    assert_eq!(
        deployment["spec"]["template"]["spec"]["containers"][0]["image"],
        "example.invalid/sift:1"
    );
}
