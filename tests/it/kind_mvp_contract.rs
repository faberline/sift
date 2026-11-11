use std::path::Path;

const SCRIPT: &str = include_str!("../../scripts/kind-mvp.sh");
const KIND_WORKFLOW: &str = include_str!("../../.github/workflows/sift-kind.yml");
const CI_WORKFLOW: &str = include_str!("../../.github/workflows/ci.yml");

#[test]
fn kind_gate_is_a_local_three_worker_preflight() {
    for required in [
        "disableDefaultCNI: true",
        "POD_SUBNET=\"${SIFT_KIND_POD_SUBNET:-10.244.0.0/16}\"",
        "name: CALICO_IPV4POOL_CIDR",
        "name: CALICO_IPV4POOL_IPIP",
        "name: CALICO_IPV4POOL_VXLAN",
        "CALICO_VERSION=\"${SIFT_KIND_CALICO_VERSION:-v3.32.1}\"",
        "CALICO_SHA256=\"${SIFT_KIND_CALICO_SHA256:-a1df919d9721cf667accdc3e72848911b0cb25cfab7d2478ad0c996302c95744}\"",
        "kind load docker-image",
        "k8s instance render --profile dev",
        r#"auth:"kubernetes""#,
        "wait_role_ready statefulset sift-store 3",
        "acceptance-grpc",
        "prometheus/api/v1/write",
        "auth_curl_status()",
        "rw2_status=\"$(auth_curl_status",
        "api/v1/correlate",
        "api/v1/logs/tail",
        "mcp-session-id",
        "extract_sse_json()",
        "mcp-init.sse",
        "mcp-tools.sse",
        "pvc-before-restart.json",
        "collector-before-restart.json",
        "collector-after-restart.json",
        "raft-leader-before.json",
        "raft-leader-after.json",
        "network-policy-probe",
        "http://sift-store.${NAMESPACE}.svc.cluster.local:7380/healthz 2>/dev/null; then",
    ] {
        assert!(SCRIPT.contains(required), "missing kind gate contract: {required}");
    }

    assert_eq!(SCRIPT.matches("- role: worker").count(), 3);
    assert!(!SCRIPT.contains("gcloud"));
    assert!(!SCRIPT.contains("terraform"));
    assert!(!SCRIPT.contains("fqdnnetworkpolicies.networking.gke.io"));
}

#[test]
fn kind_workflow_is_manual_and_not_in_the_gate() {
    for required in [
        "workflow_dispatch:",
        "scripts/kind-mvp.sh",
        "SIFT_KIND_EVIDENCE_DIR",
        "actions/upload-artifact@",
        "if: always()",
        "kind-linux-amd64.sha256sum",
    ] {
        assert!(
            KIND_WORKFLOW.contains(required),
            "sift-kind workflow is missing: {required}"
        );
    }

    for forbidden in ["pull_request", "push:", "schedule:"] {
        assert!(
            !KIND_WORKFLOW.contains(forbidden),
            "sift-kind workflow must stay manual, found: {forbidden}"
        );
    }

    assert!(
        !CI_WORKFLOW.contains("sift-kind"),
        "ci.yml must not run the sift-kind workflow"
    );
    assert!(
        !CI_WORKFLOW.contains("kind-mvp"),
        "ci.yml must not run the Kind journey in gate"
    );
}

#[test]
fn kind_gate_is_executable_and_cargo_registered() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/kind-mvp.sh");
    let mode = std::fs::metadata(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .permissions();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            mode.mode() & 0o111,
            0,
            "{} is not executable",
            path.display()
        );
    }
}
