//! The collector agent: its DaemonSet, the project role and binding it writes
//! with, and the projected client token it presents.

use serde_json::{json, Value};
use service_k8s::render::{self, RenderCtx};

use crate::operations::domain::instance_names::{AGENT_COMPONENT, HTTP_PORT};
use crate::operations::infrastructure::k8s::role_workloads::{
    role_extra_labels, role_name, role_selector_labels,
};
use crate::shared_kernel::instance_spec::{InstanceAuth as AuthMode, InstanceSettings as SiftSpec};

/// The value of `--namespace`: the listed namespaces joined by commas, or the
/// instance namespace when the list is empty.
fn log_namespaces(cx: &RenderCtx<'_>, spec: &SiftSpec) -> String {
    if spec.logs.namespaces.is_empty() {
        cx.ns().to_string()
    } else {
        spec.logs.namespaces.join(",")
    }
}

pub(super) fn agent_daemon_set_plan(cx: &RenderCtx<'_>, spec: &SiftSpec) -> render::DaemonSetPlan {
    let name = role_name(cx, AGENT_COMPONENT);
    let mut env = vec![
        json!({"name":"SIFT_DATA_DIR", "value":"/var/lib/sift"}),
        json!({"name":"SIFT_URL", "value":format!("http://{}:{HTTP_PORT}", cx.name())}),
        json!({"name":"NODE_NAME", "valueFrom":{"fieldRef":{"fieldPath":"spec.nodeName"}}}),
    ];
    if matches!(spec.auth, AuthMode::Required) {
        if let Some(secret) = &spec.tokens_secret {
            env.push(json!({
                "name":"SIFT_TOKEN",
                "valueFrom":{"secretKeyRef":{"name":secret,"key":"agent-token"}}
            }));
        }
    }
    if matches!(spec.auth, AuthMode::Kubernetes) {
        env.extend([
            json!({"name":"SIFT_TOKEN_FILE", "value":"/var/run/secrets/sift/client/token"}),
            json!({"name":"SIFT_TOKEN_AUDIENCE", "value":"sift.axiom.dev"}),
        ]);
    }
    let mut agent_mounts = vec![
        json!({"name":"pod-logs","mountPath":"/var/log/pods","readOnly":true}),
        json!({"name":"data","mountPath":"/var/lib/sift"}),
    ];
    let mut agent_volumes = vec![
        json!({"name":"pod-logs","hostPath":{"path":"/var/log/pods","type":"Directory"}}),
        json!({"name":"data","hostPath":{"path":format!("/var/lib/sift/agents/{}/{}",cx.ns(),cx.name()),"type":"DirectoryOrCreate"}}),
    ];
    if matches!(spec.auth, AuthMode::Kubernetes) {
        agent_mounts.push(json!({
            "name":"sift-client-token",
            "mountPath":"/var/run/secrets/sift/client",
            "readOnly":true
        }));
        agent_volumes.push(json!({
            "name":"sift-client-token",
            "projected":{
                "defaultMode": 420,
                "sources":[{"serviceAccountToken":{
                    "audience":"sift.axiom.dev",
                    "expirationSeconds":600,
                    "path":"token"
                }}]
            }
        }));
    }
    let runtime = render::PodRuntimePolicy::restricted(
        cx.name(),
        serde_json::to_value(&spec.placement.node_selector).expect("node selector is JSON"),
    )
    .with_init_containers(vec![json!({
        "name":"prepare-data-root",
        "image":"busybox:1.36.1",
        "command":["sh","-ec"],
        "args":["mkdir -p /var/lib/sift/agent && chown -R 65532:65532 /var/lib/sift && chmod 0700 /var/lib/sift /var/lib/sift/agent"],
        "volumeMounts":[{"name":"data","mountPath":"/var/lib/sift"}],
        "securityContext":{
            "runAsNonRoot":false,"runAsUser":0,"runAsGroup":0,
            "allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,
            "capabilities":{"drop":["ALL"],"add":["CHOWN","FOWNER","DAC_OVERRIDE"]}
        }
    })])
    .with_volumes(agent_volumes);
    // The agent lists Pods through the Kubernetes API only when a Pod label is set.
    let runtime = if spec.logs.pod_label.is_some() {
        runtime.with_automount_service_account_token(true)
    } else {
        runtime
    };
    let mut args: Vec<String> = vec![
        "collect".into(),
        "--cri-root".into(),
        "/var/log/pods".into(),
        "--namespace".into(),
        log_namespaces(cx, spec),
        "--data-dir".into(),
        "/var/lib/sift".into(),
        "--checkpoint".into(),
        "/var/lib/sift/agent/checkpoint.json".into(),
        "--quarantine".into(),
        "/var/lib/sift/agent/rejected.jsonl".into(),
        "--project".into(),
        cx.name().into(),
        "--environment".into(),
        cx.ns().into(),
        "--gcp-project".into(),
        spec.gcp_project_id.clone(),
        "--cluster".into(),
        spec.gke_cluster_name.clone(),
        "--location".into(),
        spec.gke_location.clone(),
        "--node".into(),
        "$(NODE_NAME)".into(),
        "--follow".into(),
    ];
    if let Some(label) = &spec.logs.pod_label {
        args.push("--pod-label".into());
        args.push(label.clone());
    }
    let mut container = render::ContainerPlan::new("sift", &spec.image, args);
    container.env = env;
    container.volume_mounts = agent_mounts;
    container.security_context = Some(json!({
        "runAsNonRoot":false,"runAsUser":0,
        "allowPrivilegeEscalation":false,"readOnlyRootFilesystem":true,
        "capabilities":{"drop":["ALL"],"add":["DAC_OVERRIDE"]}
    }));
    container.resources = Some(
        json!({"requests":{"cpu":"25m","memory":"64Mi"},"limits":{"cpu":"500m","memory":"256Mi"}}),
    );
    render::DaemonSetPlan::new(
        name,
        render::PodPlan::new(AGENT_COMPONENT, container, runtime)
            .with_selector_labels(role_selector_labels(AGENT_COMPONENT))
            .with_labels(role_extra_labels(AGENT_COMPONENT)),
    )
}

pub(super) fn agent_project_role_plan(cx: &RenderCtx<'_>) -> render::RolePlan {
    let name = format!("{}-agent-project", cx.name());
    render::RolePlan::new(name, "auth")
        .with_rule(
            render::RbacRulePlan::new(
                vec!["sift.axiom.dev".into()],
                vec!["projects".into()],
                vec!["get".into(), "create".into()],
            )
            .with_resource_names(vec![cx.name().into()]),
        )
        .with_rule(render::RbacRulePlan::new(
            vec!["".into()],
            vec!["pods".into()],
            vec!["get".into(), "list".into()],
        ))
}

pub(super) fn projected_client_token_volume() -> Value {
    json!({
        "name":"sift-client-token",
        "projected":{
            "defaultMode":420,
            "sources":[{"serviceAccountToken":{
                "audience":"sift.axiom.dev",
                "expirationSeconds":600,
                "path":"token"
            }}]
        }
    })
}

pub(super) fn projected_client_token_mount() -> Value {
    json!({
        "name":"sift-client-token",
        "mountPath":"/var/run/secrets/sift/client",
        "readOnly":true
    })
}

pub(super) fn agent_project_role_binding_plan(cx: &RenderCtx<'_>) -> render::RoleBindingPlan {
    let name = format!("{}-agent-project", cx.name());
    render::RoleBindingPlan::new(name.clone(), "auth", name)
        .with_service_account(render::ServiceAccountSubjectPlan::new(cx.ns(), cx.name()))
}
