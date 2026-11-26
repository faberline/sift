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

const GATEWAY_DIR: &str = "k8s/components/gateway-api-public";

/// Every `(type, value)` of `spec.rules[].matches[].path`. The default type is `PathPrefix`.
fn route_paths(document: &Value) -> Vec<(String, String)> {
    document["spec"]["rules"]
        .as_sequence()
        .into_iter()
        .flatten()
        .flat_map(|rule| rule["matches"].as_sequence().into_iter().flatten())
        .filter_map(|matched| {
            let path = &matched["path"];
            let value = path["value"].as_str()?.to_string();
            let kind = path["type"].as_str().unwrap_or("PathPrefix").to_string();
            Some((kind, value))
        })
        .collect()
}

/// True when one rule routes `request`. `PathPrefix` matches whole path elements.
fn route_covers(paths: &[(String, String)], request: &str) -> bool {
    paths.iter().any(|(kind, value)| match kind.as_str() {
        "Exact" => value == request,
        "PathPrefix" => {
            value == "/" || request == value || request.starts_with(&format!("{value}/"))
        }
        _ => false,
    })
}

#[test]
fn gateway_api_component_lists_both_routes() {
    let kustomization = documents(GATEWAY_DIR, "kustomization.yaml");
    assert_eq!(kustomization.len(), 1);
    assert_eq!(kustomization[0]["kind"].as_str(), Some("Component"));
    assert_eq!(
        resources(GATEWAY_DIR),
        strings(&["httproute.yaml", "grpcroute.yaml"])
    );
}

#[test]
fn gateway_api_httproute_reaches_the_gateway_on_the_http_port() {
    let routes = documents(GATEWAY_DIR, "httproute.yaml");
    assert_eq!(routes.len(), 1);
    let route = &routes[0];
    assert_eq!(route["kind"].as_str(), Some("HTTPRoute"));
    assert_eq!(
        route["spec"]["parentRefs"][0]["name"].as_str(),
        Some("REPLACE_ME__SIFT_GATEWAY_NAME")
    );
    assert!(route["metadata"]["namespace"].is_null());
    let rules = route["spec"]["rules"].as_sequence().expect("rules");
    assert!(
        rules.len() >= 4,
        "expected at least 4 rules, got {}",
        rules.len()
    );
    for rule in rules {
        let backend = &rule["backendRefs"][0];
        assert_eq!(backend["name"].as_str(), Some("sift-gateway"));
        assert_eq!(backend["port"].as_u64(), Some(7380));
    }
}

#[test]
fn gateway_api_httproute_exposes_ingest_query_and_management_paths() {
    let route = &documents(GATEWAY_DIR, "httproute.yaml")[0];
    let paths = route_paths(route);
    for request in [
        "/v1/logs",
        "/v1/metrics",
        "/v1/traces",
        "/v1/traces/abc",
        "/prometheus/api/v1/write",
        "/api/v1/query",
        "/api/v2/query",
        "/api/v1/logs/tail",
        "/api/v1/traces/abc",
        "/api/v1/correlate",
        "/api/v1/services",
        "/api/v1/queries/abc",
        "/prometheus/api/v1/query",
        "/prometheus/api/v1/query_range",
        "/api/v1/saved-queries/abc",
        "/api/v1/error-groups/abc",
        "/api/v1/alert-rules/abc",
        "/api/v1/alerts/abc/mute",
        "/readyz",
    ] {
        assert!(route_covers(&paths, request), "not routed: {request}");
    }
}

#[test]
fn gateway_api_httproute_does_not_expose_mcp_admin_or_recovery() {
    let route = &documents(GATEWAY_DIR, "httproute.yaml")[0];
    let paths = route_paths(route);
    for request in [
        "/mcp",
        "/admin/backup",
        "/admin/integrity",
        "/api/v1/recovery/data",
        "/api",
        "/",
        "/api/v1",
    ] {
        assert!(!route_covers(&paths, request), "must not route: {request}");
    }
}

#[test]
fn gateway_api_grpcroute_reaches_the_gateway_on_the_otlp_port() {
    let routes = documents(GATEWAY_DIR, "grpcroute.yaml");
    assert_eq!(routes.len(), 1);
    let route = &routes[0];
    assert_eq!(route["kind"].as_str(), Some("GRPCRoute"));
    assert_eq!(
        route["spec"]["parentRefs"][0]["name"].as_str(),
        Some("REPLACE_ME__SIFT_GATEWAY_NAME")
    );
    let rules = route["spec"]["rules"].as_sequence().expect("rules");
    assert_eq!(rules.len(), 1);
    let backend = &rules[0]["backendRefs"][0];
    assert_eq!(backend["name"].as_str(), Some("sift-gateway"));
    assert_eq!(backend["port"].as_u64(), Some(4317));
    let mut services: Vec<String> = rules[0]["matches"]
        .as_sequence()
        .expect("matches")
        .iter()
        .map(|matched| {
            assert_eq!(matched["method"]["method"].as_str(), Some("Export"));
            matched["method"]["service"]
                .as_str()
                .expect("service")
                .to_string()
        })
        .collect();
    services.sort();
    let mut expected = strings(&[
        "opentelemetry.proto.collector.logs.v1.LogsService",
        "opentelemetry.proto.collector.trace.v1.TraceService",
        "opentelemetry.proto.collector.metrics.v1.MetricsService",
    ]);
    expected.sort();
    assert_eq!(services, expected);
}

#[test]
fn gateway_api_docs_table_names_the_paths_that_stay_internal() {
    let docs = read("docs/operations/public-tls.md");
    assert!(docs.contains("## Gateway API variant"));
    for path in ["/mcp", "/admin/backup", "/api/v1/recovery/data"] {
        assert!(docs.contains(path), "docs must name {path}");
    }
}
