//! Namespace-scoped RBAC for the operator (issue #389).
//!
//! These checks read the checked-in manifests. They run no kubectl.

use std::path::Path;

use serde::Deserialize;
use serde_yaml::Value;

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn parse_yaml_documents(body: &str) -> Vec<Value> {
    serde_yaml::Deserializer::from_str(body)
        .map(|document| Value::deserialize(document).expect("parse Kubernetes YAML document"))
        .filter(|document| !document.is_null())
        .collect()
}

fn document(relative: &str) -> Value {
    let mut documents = parse_yaml_documents(&read(relative));
    assert_eq!(documents.len(), 1, "{relative} holds one document");
    documents.remove(0)
}

fn rules(relative: &str) -> Vec<Value> {
    document(relative)["rules"]
        .as_sequence()
        .expect("rules list")
        .clone()
}

fn strings_of(value: &Value) -> Vec<String> {
    value
        .as_sequence()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn granted_resources(relative: &str) -> Vec<String> {
    rules(relative)
        .iter()
        .flat_map(|rule| strings_of(&rule["resources"]))
        .collect()
}

#[test]
fn namespace_scoped_cluster_role_has_no_rbac_write() {
    let file = "k8s/operator/namespace-scoped/clusterrole.yaml";
    assert_eq!(document(file)["kind"].as_str(), Some("ClusterRole"));

    for rule in rules(file) {
        let groups = strings_of(&rule["apiGroups"]);
        if !groups
            .iter()
            .any(|group| group == "rbac.authorization.k8s.io")
        {
            continue;
        }
        assert_eq!(groups, ["rbac.authorization.k8s.io"], "rule: {rule:?}");
        assert_eq!(
            strings_of(&rule["resources"]),
            ["clusterrolebindings"],
            "only the read of clusterrolebindings is allowed: {rule:?}"
        );
        assert_eq!(
            strings_of(&rule["verbs"]),
            ["get"],
            "RBAC objects are read only: {rule:?}"
        );
    }

    let resources = granted_resources(file);
    for refused in ["roles", "rolebindings", "clusterroles"] {
        assert!(
            !resources.iter().any(|resource| resource == refused),
            "{refused} must not be granted cluster-wide"
        );
    }
    assert!(
        resources.iter().any(|resource| resource == "sifts"),
        "the sift rule is present"
    );
}

#[test]
fn namespace_scoped_cluster_role_reads_the_precreated_binding() {
    let file = "k8s/operator/namespace-scoped/clusterrole.yaml";
    let found = rules(file).into_iter().any(|rule| {
        strings_of(&rule["apiGroups"]) == ["rbac.authorization.k8s.io"]
            && strings_of(&rule["resources"]) == ["clusterrolebindings"]
            && strings_of(&rule["verbs"]) == ["get"]
    });
    assert!(found, "the get rule on clusterrolebindings is missing");
}

#[test]
fn namespaced_role_holds_the_workload_rules() {
    let role_file = "k8s/operator/namespaced-rbac/role.yaml";
    let role = document(role_file);
    assert_eq!(role["kind"].as_str(), Some("Role"));
    assert_eq!(role["metadata"]["name"].as_str(), Some("sift-operator"));
    assert!(
        role["metadata"]["namespace"].is_null(),
        "the overlay sets the namespace"
    );

    let resources = granted_resources(role_file);
    for needed in [
        "statefulsets",
        "deployments",
        "networkpolicies",
        "rolebindings",
        "roles",
        "cronjobs",
    ] {
        assert!(
            resources.iter().any(|resource| resource == needed),
            "{needed} is missing from the Role"
        );
    }

    let binding = document("k8s/operator/namespaced-rbac/rolebinding.yaml");
    assert_eq!(binding["kind"].as_str(), Some("RoleBinding"));
    assert_eq!(binding["roleRef"]["kind"].as_str(), Some("Role"));
    assert_eq!(binding["roleRef"]["name"].as_str(), Some("sift-operator"));
    let subjects = binding["subjects"].as_sequence().expect("subjects list");
    assert_eq!(subjects.len(), 1, "one subject");
    assert_eq!(subjects[0]["name"].as_str(), Some("sift-operator"));
    assert_eq!(subjects[0]["namespace"].as_str(), Some("sift-system"));

    let base = document("k8s/operator/namespaced-rbac/kustomization.yaml");
    assert_eq!(
        strings_of(&base["resources"]),
        vec!["role.yaml", "rolebinding.yaml"]
    );
    assert!(base["namespace"].is_null(), "the base sets no namespace");

    let example = document("k8s/operator/namespaced-rbac/example/kustomization.yaml");
    assert_eq!(example["namespace"].as_str(), Some("sift-example"));
    assert_eq!(strings_of(&example["resources"]), vec![".."]);
}

#[test]
fn cluster_wide_default_is_unchanged() {
    let resources = granted_resources("k8s/operator/cluster/clusterrole.yaml");
    assert!(
        resources
            .iter()
            .any(|resource| resource == "clusterrolebindings"),
        "the cluster-wide ClusterRole keeps its clusterrolebindings rule"
    );

    let base = document("k8s/operator/kustomization.yaml");
    assert_eq!(
        strings_of(&base["resources"]),
        vec!["cluster", "namespace", "namespaced"]
    );
}

/// The `(name, value)` pairs of the env of the container `operator`.
/// `value` is `None` when the entry has no string `value`.
fn operator_env(deployment: &Value) -> Vec<(String, Option<String>)> {
    let containers = deployment["spec"]["template"]["spec"]["containers"]
        .as_sequence()
        .expect("containers list");
    let operator = containers
        .iter()
        .find(|container| container["name"].as_str() == Some("operator"))
        .expect("operator container");
    operator["env"]
        .as_sequence()
        .expect("env list")
        .iter()
        .map(|entry| {
            (
                entry["name"].as_str().expect("env name").to_string(),
                entry["value"].as_str().map(str::to_string),
            )
        })
        .collect()
}

#[test]
fn operator_deployment_sets_the_watch_namespaces() {
    let patch =
        document("k8s/operator/namespace-scoped/watch-namespaces/watch-namespaces-patch.yaml");
    assert_eq!(patch["kind"].as_str(), Some("Deployment"));
    assert_eq!(patch["metadata"]["name"].as_str(), Some("sift-operator"));
    assert!(
        operator_env(&patch).contains(&(
            "WATCH_NAMESPACES".to_string(),
            Some("sift-example".to_string())
        )),
        "the patch sets WATCH_NAMESPACES to the watched namespaces"
    );

    let overlay = document("k8s/operator/namespace-scoped/watch-namespaces/kustomization.yaml");
    assert_eq!(strings_of(&overlay["resources"]), vec!["../../namespaced"]);
    assert_eq!(
        overlay["patches"][0]["path"].as_str(),
        Some("watch-namespaces-patch.yaml")
    );
}

#[test]
fn operator_deployment_defaults_to_cluster_wide_watch() {
    let env = operator_env(&document("k8s/operator/namespaced/deployment.yaml"));
    assert!(
        env.contains(&("WATCH_NAMESPACES".to_string(), Some(String::new()))),
        "the base sets WATCH_NAMESPACES to an empty value (cluster-wide)"
    );
    for name in ["POD_NAMESPACE", "POD_NAME"] {
        assert!(
            env.iter().any(|(entry, _)| entry == name),
            "{name} is missing from the operator env"
        );
    }
}

#[test]
fn operator_reads_the_watch_namespaces_variable() {
    let source = read("src/operations/interfaces/operator.rs");
    assert!(
        source.contains("\"WATCH_NAMESPACES\""),
        "the operator names the WATCH_NAMESPACES variable"
    );
    assert!(
        source.contains("run_in_namespaces"),
        "the operator calls run_in_namespaces"
    );
    assert!(
        !source.contains("service_k8s::run::<"),
        "the operator no longer calls the cluster-wide run"
    );
}
