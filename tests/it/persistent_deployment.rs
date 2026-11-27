//! Deployment contract for Sift's common `/var/lib/sift` durable root.

use std::{fs, path::PathBuf, process::Command};

use serde_json::{json, Value};
use service_k8s::ManagedService;
use sift::operations::Sift;

fn sift_resource() -> Sift {
    sift_resource_with(json!({}))
}

/// The test Sift resource with the keys of `extra` merged into its spec.
fn sift_resource_with(extra: Value) -> Sift {
    let mut spec = json!({
        "image": "example.invalid/sift:test",
        "peerTlsSecret": "sift-peer-tls",
        "dataSize": "10Gi",
        "auth": "off"
    });
    if let (Some(spec), Some(extra)) = (spec.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            spec.insert(key.clone(), value.clone());
        }
    }
    serde_json::from_value(json!({
        "apiVersion": "sift.axiom.dev/v1alpha1",
        "kind": "Sift",
        "metadata": {
            "name": "events",
            "namespace": "observability",
            "uid": "sift-owner-uid"
        },
        "spec": spec
    }))
    .expect("decode Sift resource")
}

/// The workloads that run the data-root init container: only the log agent.
const INIT_CONTAINER_WORKLOADS: [(&str, &str); 1] = [("DaemonSet", "events-agent")];

const DEFAULT_INIT_IMAGE: &str =
    "busybox:1.36.1@sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662";
const OVERRIDE_INIT_IMAGE: &str = "registry.example.invalid/mirror/busybox@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn default_init_containers_pin_busybox_by_digest() {
    let objects = sift_resource().render();

    for (kind, name) in INIT_CONTAINER_WORKLOADS {
        assert_eq!(
            object(&objects, kind, name)["spec"]["template"]["spec"]["initContainers"][0]["image"],
            DEFAULT_INIT_IMAGE,
            "{kind} {name} must pin the init image by digest"
        );
    }
    for (kind, name) in [
        ("StatefulSet", "events-store"),
        ("StatefulSet", "events-control"),
        ("Deployment", "events-gateway"),
        ("Deployment", "events-query"),
    ] {
        assert!(
            object(&objects, kind, name)["spec"]["template"]["spec"]
                .get("initContainers")
                .is_none(),
            "{kind} {name} must not run an init container"
        );
    }
}

#[test]
fn init_image_overrides_every_init_container() {
    let objects = sift_resource_with(json!({ "initImage": OVERRIDE_INIT_IMAGE })).render();

    for (kind, name) in INIT_CONTAINER_WORKLOADS {
        assert_eq!(
            object(&objects, kind, name)["spec"]["template"]["spec"]["initContainers"][0]["image"],
            OVERRIDE_INIT_IMAGE,
            "{kind} {name} must use spec.initImage"
        );
    }
}

#[test]
fn spec_without_init_image_decodes() {
    let sift = sift_resource();
    assert!(sift.spec.init_image.is_none());
}

fn object<'a>(objects: &'a [Value], kind: &str, name: &str) -> &'a Value {
    objects
        .iter()
        .find(|object| object["kind"] == kind && object["metadata"]["name"] == name)
        .unwrap_or_else(|| panic!("missing {kind}/{name}"))
}

#[test]
fn rendered_role_pods_pass_restricted_pod_security() {
    let topology = sift_resource().render();
    let single_node = sift_resource_with(json!({"topology": "singleNode"})).render();
    let cases: Vec<(&Vec<Value>, &str, &str)> = vec![
        (&topology, "StatefulSet", "events-store"),
        (&topology, "StatefulSet", "events-control"),
        (&topology, "Deployment", "events-gateway"),
        (&topology, "Deployment", "events-query"),
        (&single_node, "StatefulSet", "events-all"),
    ];
    for (objects, kind, name) in cases {
        let pod = &object(objects, kind, name)["spec"]["template"]["spec"];
        assert_eq!(pod["securityContext"]["runAsNonRoot"], true, "{name}");
        assert_eq!(
            pod["securityContext"]["seccompProfile"]["type"], "RuntimeDefault",
            "{name}"
        );
        assert_eq!(pod["securityContext"]["fsGroup"], 65532, "{name}");
        assert!(pod.get("initContainers").is_none(), "{name}");
        for key in ["hostNetwork", "hostPID", "hostIPC"] {
            assert!(
                pod.get(key).is_none_or(|value| value == &json!(false)),
                "{name} {key}"
            );
        }
        for volume in pod
            .get("volumes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            assert!(volume.get("hostPath").is_none(), "{name}");
        }
        let containers = pod["containers"].as_array().expect("containers");
        assert!(!containers.is_empty(), "{name}");
        for container in containers {
            let context = &container["securityContext"];
            assert_eq!(context["allowPrivilegeEscalation"], false, "{name}");
            assert_eq!(context["capabilities"]["drop"], json!(["ALL"]), "{name}");
            assert!(context["capabilities"].get("add").is_none(), "{name}");
            assert_eq!(context["runAsNonRoot"], true, "{name}");
            assert_ne!(context["runAsUser"], 0, "{name}");
        }
    }
}

#[test]
fn every_runtime_image_prepares_a_private_nonroot_volume() {
    for (name, dockerfile, stage) in [
        ("source", include_str!("../../Dockerfile"), "build"),
        (
            "release",
            include_str!("../../Dockerfile.release"),
            "binary-source",
        ),
        ("test", include_str!("../../Dockerfile.test"), "data-root"),
    ] {
        assert!(
            dockerfile.contains("SIFT_DATA_DIR=/var/lib/sift"),
            "{name} image must use the shared data root"
        );
        assert!(
            dockerfile.contains("VOLUME [\"/var/lib/sift\"]"),
            "{name} image must declare a durable volume"
        );
        assert!(
            dockerfile.contains(&format!(
                "COPY --from={stage} --chown=65532:65532 --chmod=0700 /image-root/var/lib/sift /var/lib/sift"
            )),
            "{name} image must make the mounted root writable by its nonroot runtime"
        );
    }
}

#[test]
fn the_official_compose_example_uses_a_named_volume() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("compose.yaml");
    let body = fs::read_to_string(path).expect("official compose example must exist");
    let compose: serde_yaml::Value = serde_yaml::from_str(&body).expect("parse compose yaml");
    assert_eq!(
        compose["services"]["sift"]["volumes"][0],
        "sift-data:/var/lib/sift"
    );
    assert!(compose["volumes"]["sift-data"].is_mapping());
}

#[test]
fn the_official_compose_example_builds_from_the_repository_root() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("compose.yaml");
    let body = fs::read_to_string(path).expect("official compose example must exist");
    let compose: serde_yaml::Value = serde_yaml::from_str(&body).expect("parse compose yaml");
    assert_eq!(compose["services"]["sift"]["build"]["context"], ".");
}

/// Returns the problems in the `## Quickstart` section of `readme`: required
/// items it does not name, required or linked paths that do not exist under
/// `root`. Empty when the section is sound.
fn quickstart_missing_paths(readme: &str, root: &std::path::Path) -> Vec<String> {
    const REQUIRED: [&str; 8] = [
        "install.sh",
        "Dockerfile",
        "compose.yaml",
        "k8s/overlays/template",
        "k8s/crd",
        "k8s/operator",
        "docs/operations/ha.md",
        "docs/pilot.md",
    ];
    let start = readme
        .lines()
        .position(|line| line == "## Quickstart")
        .unwrap_or_else(|| panic!("README has no ## Quickstart section"));
    let section = readme
        .lines()
        .skip(start + 1)
        .take_while(|line| !line.starts_with("## "))
        .collect::<Vec<_>>()
        .join("\n");

    let mut missing = Vec::new();
    for item in REQUIRED {
        if !section.contains(item) {
            missing.push(format!("missing from quickstart: {item}"));
        }
        if !root.join(item).exists() {
            missing.push(item.to_string());
        }
    }
    let mut rest = section.as_str();
    while let Some(at) = rest.find("](") {
        let after = &rest[at + 2..];
        let Some(end) = after.find(')') else { break };
        let target = &after[..end];
        rest = &after[end + 1..];
        if target.starts_with("http") || target.starts_with('#') {
            continue;
        }
        let path = target.split('#').next().unwrap_or(target);
        if !root.join(path).exists() {
            missing.push(target.to_string());
        }
    }
    missing.sort();
    missing.dedup();
    missing
}

#[test]
fn the_readme_quickstart_names_only_existing_paths() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("README.md")).expect("README must exist");
    let missing = quickstart_missing_paths(&readme, &root);
    assert!(missing.is_empty(), "quickstart problems: {missing:?}");
}

#[test]
fn the_quickstart_check_rejects_a_missing_path() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let readme = "## Quickstart\nsee [x](scripts/does-not-exist.sh)\n## Next\n";
    let missing = quickstart_missing_paths(readme, &root);
    assert!(
        missing.contains(&"scripts/does-not-exist.sh".to_string()),
        "expected the missing link target, got {missing:?}"
    );
}

#[test]
fn every_operator_role_uses_the_common_private_persistent_root() {
    let objects = sift_resource().render();
    for (kind, name, role) in [
        ("StatefulSet", "events-store", "store"),
        ("StatefulSet", "events-control", "control"),
        ("Deployment", "events-gateway", "gateway"),
        ("Deployment", "events-query", "query"),
    ] {
        let workload = object(&objects, kind, name);
        let pod = &workload["spec"]["template"]["spec"];
        assert_eq!(pod["securityContext"]["fsGroup"], 65532, "{name}");
        assert_eq!(
            pod["securityContext"]["fsGroupChangePolicy"], "OnRootMismatch",
            "{name}"
        );
        assert!(!pod.to_string().contains("emptyDir"), "{name}");
        assert!(pod.get("initContainers").is_none(), "{name}");
        let container = &pod["containers"][0];
        assert_eq!(container["volumeMounts"][0]["mountPath"], "/var/lib/sift");
        let expected_args = if matches!(role, "store" | "control") {
            json!([
                "serve",
                "--role",
                role,
                "--data-dir",
                "/var/lib/sift",
                "--retention-days",
                "180"
            ])
        } else {
            json!(["serve", "--role", role, "--data-dir", "/var/lib/sift"])
        };
        assert_eq!(container["args"], expected_args);
        if kind == "StatefulSet" {
            assert_eq!(
                workload["spec"]["volumeClaimTemplates"][0]["metadata"]["name"],
                "data"
            );
        } else {
            let pvc_name = format!("{name}-data");
            object(&objects, "PersistentVolumeClaim", &pvc_name);
            assert_eq!(
                pod["volumes"][0]["persistentVolumeClaim"]["claimName"],
                pvc_name
            );
        }
    }

    assert_eq!(
        object(&objects, "StatefulSet", "events-store")["spec"]["replicas"],
        3
    );
    assert_eq!(
        object(&objects, "StatefulSet", "events-control")["spec"]["replicas"],
        3
    );
    let agent = object(&objects, "DaemonSet", "events-agent");
    let agent_pod = &agent["spec"]["template"]["spec"];
    assert_eq!(
        agent_pod["securityContext"]["fsGroupChangePolicy"],
        "OnRootMismatch"
    );
    assert!(agent.to_string().contains("/var/lib/sift/agent"));
    assert!(!agent.to_string().contains("emptyDir"));
    assert!(
        agent_pod["initContainers"][0]["securityContext"]["capabilities"]["add"]
            .as_array()
            .is_some_and(|capabilities| capabilities.contains(&json!("DAC_OVERRIDE")))
    );
    assert_eq!(
        agent_pod["containers"][0]["securityContext"]["capabilities"]["add"],
        json!(["DAC_OVERRIDE"])
    );
}

#[test]
fn the_agent_keeps_checkpoints_below_the_common_root() {
    let output = Command::new(env!("CARGO_BIN_EXE_sift"))
        .args([
            "k8s",
            "collector",
            "render",
            "--namespace",
            "observability",
            "--image",
            "example.invalid/sift:test",
        ])
        .output()
        .expect("render collector");
    assert!(output.status.success());
    let yaml = String::from_utf8(output.stdout).expect("collector yaml is utf-8");
    assert!(yaml.contains("path: /var/lib/sift"));
    assert!(yaml.contains("mountPath: /var/lib/sift"));
    assert!(yaml.contains("/var/lib/sift/agent/checkpoint.json"));
    assert!(yaml.contains("/var/lib/sift/agent/rejected.jsonl"));
    assert!(!yaml.contains("/var/lib/sift-collector"));
}
