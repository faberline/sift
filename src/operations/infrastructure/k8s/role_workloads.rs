//! The store, control, query and gateway roles: their names and labels, pod and
//! container plans, environment, volumes, Services, StatefulSets, Deployments
//! and disruption budgets.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use service_k8s::render::{self, RenderCtx};

use crate::operations::infrastructure::k8s::constants::{
    APP, CONTROL_COMPONENT, GATEWAY_COMPONENT, HTTP_PORT, OTLP_GRPC_PORT, PEER_MTLS_PORT,
    QUERY_COMPONENT, STORE_COMPONENT,
};
use crate::operations::interfaces::crd::sift_spec::{AuthMode, SiftSpec};

pub(super) fn role_name(cx: &RenderCtx<'_>, role: &str) -> String {
    format!("{}-{role}", cx.name)
}

pub(super) fn auth_delegator_binding_name(cx: &RenderCtx<'_>) -> String {
    // Namespace names cannot contain dots, so this mapping cannot collide
    // between (namespace, instance) pairs that contain dashes.
    format!("sift.{}.{}.auth-delegator", cx.ns, cx.name)
}

fn storage_size<'a>(spec: &'a SiftSpec, role: &str) -> &'a str {
    if let Some(legacy) = spec.data_size.as_deref() {
        return legacy;
    }
    match role {
        STORE_COMPONENT => &spec.storage.store_size,
        CONTROL_COMPONENT => &spec.storage.control_size,
        GATEWAY_COMPONENT => &spec.storage.gateway_size,
        QUERY_COMPONENT => &spec.storage.query_size,
        _ => unreachable!("role {role} does not own a Sift PVC"),
    }
}

pub(super) fn role_selector_labels(role: &str) -> BTreeMap<String, String> {
    BTreeMap::from([("sift.axiom.dev/role".to_string(), role.to_string())])
}

pub(super) fn role_extra_labels(role: &str) -> BTreeMap<String, String> {
    let mut labels = role_selector_labels(role);
    if matches!(role, GATEWAY_COMPONENT | QUERY_COMPONENT) {
        labels.insert("sift.axiom.dev/frontend".to_string(), "true".to_string());
    }
    labels
}

pub(super) fn full_role_selector(cx: &RenderCtx<'_>, role: &str) -> BTreeMap<String, String> {
    let mut selector = BTreeMap::from([
        ("app.kubernetes.io/name".to_string(), APP.to_string()),
        (
            "app.kubernetes.io/instance".to_string(),
            cx.name.to_string(),
        ),
        ("app.kubernetes.io/component".to_string(), role.to_string()),
    ]);
    selector.extend(role_selector_labels(role));
    selector
}

pub(super) fn container_security_context() -> Value {
    json!({
        "runAsNonRoot": true,
        "runAsUser": 65532,
        "runAsGroup": 65532,
        "allowPrivilegeEscalation": false,
        "readOnlyRootFilesystem": true,
        "capabilities": {"drop": ["ALL"]},
    })
}

fn data_init_container() -> Value {
    json!({
        "name": "prepare-data-root",
        "image": "busybox:1.36.1",
        "command": ["sh", "-ec"],
        "args": ["chown 65532:65532 /var/lib/sift && chmod 0700 /var/lib/sift && test \"$(stat -c '%u:%g:%a' /var/lib/sift)\" = '65532:65532:700'"],
        "volumeMounts": [{"name":"data", "mountPath":"/var/lib/sift"}],
        "securityContext": {
            "runAsNonRoot": false,
            "runAsUser": 0,
            "runAsGroup": 0,
            "allowPrivilegeEscalation": false,
            "readOnlyRootFilesystem": true,
            "capabilities": {"drop":["ALL"], "add":["CHOWN", "FOWNER"]}
        },
        "resources": {
            "requests": {"cpu":"5m", "memory":"8Mi"},
            "limits": {"cpu":"50m", "memory":"32Mi"}
        }
    })
}

fn role_env(cx: &RenderCtx<'_>, spec: &SiftSpec, role: &str) -> Vec<Value> {
    let auth = match spec.auth {
        AuthMode::Off => "off",
        AuthMode::Required => "required",
        AuthMode::Kubernetes => "kubernetes",
    };
    let mut env = vec![
        json!({"name":"SIFT_DATA_DIR", "value":"/var/lib/sift"}),
        json!({"name":"SIFT_AUTH", "value":auth}),
        json!({"name":"SIFT_STORE_ENDPOINT", "value":format!("http://{}-store.{}.svc.cluster.local:{HTTP_PORT}", cx.name, cx.ns)}),
        json!({"name":"SIFT_STORE_GRPC_ENDPOINT", "value":format!("http://{}-store.{}.svc.cluster.local:{OTLP_GRPC_PORT}", cx.name, cx.ns)}),
        json!({"name":"SIFT_QUERY_ENDPOINT", "value":format!("http://{}-query.{}.svc.cluster.local:{HTTP_PORT}", cx.name, cx.ns)}),
        json!({"name":"SIFT_CONTROL_ENDPOINT", "value":format!("http://{}-control.{}.svc.cluster.local:{HTTP_PORT}", cx.name, cx.ns)}),
        json!({"name":"SIFT_MCP_ALLOWED_HOSTS", "value":format!("localhost,127.0.0.1,{},{}.{}.svc,{}.{}.svc.cluster.local", cx.name, cx.name, cx.ns, cx.name, cx.ns)}),
        json!({"name":"SIFT_MCP_ALLOWED_ORIGINS", "value":format!("http://{}.{}.svc.cluster.local:{HTTP_PORT}", cx.name, cx.ns)}),
        json!({"name":"SIFT_MAX_INGEST_ITEMS_PER_PROJECT_WINDOW", "value":spec.ingest.max_items_per_minute.to_string()}),
        json!({"name":"SIFT_MAX_CONCURRENT_INGEST_PER_PROJECT", "value":spec.ingest.max_concurrent_requests.to_string()}),
        json!({"name":"SIFT_MAX_EVENTS_PER_BATCH", "value":"1000"}),
        json!({"name":"SIFT_INGEST_QUOTA_WINDOW_SECS", "value":"60"}),
    ];
    if role == STORE_COMPONENT {
        env.push(json!({
            "name":"SIFT_ARCHIVE_INTERVAL_SECS",
            "value":"60"
        }));
        if let Some(archive) = &spec.archive {
            env.push(json!({
                "name":"SIFT_ARCHIVE_DESTINATION",
                "value":archive.destination
            }));
        }
        if let Some(manifest_uri) = &spec.bootstrap.archive_manifest_uri {
            env.push(json!({
                "name":"SIFT_BOOTSTRAP_ARCHIVE_MANIFEST_URI",
                "value":manifest_uri
            }));
        }
    }
    if matches!(role, STORE_COMPONENT | CONTROL_COMPONENT) {
        env.extend([
            json!({"name":"POD_NAME", "valueFrom":{"fieldRef":{"fieldPath":"metadata.name"}}}),
            json!({"name":"SHARD_COUNT", "value":"1"}),
            json!({"name":"REPLICAS_PER_SHARD", "value":"3"}),
            json!({"name":"VOTER_COUNT", "value":"3"}),
            json!({
                "name":"SIFT_RAFT_HEADLESS",
                "value":format!("{}-{role}-headless.{}.svc.cluster.local", cx.name, cx.ns)
            }),
            json!({"name":"SIFT_PEER_PORT", "value":PEER_MTLS_PORT.to_string()}),
            json!({"name":"SIFT_PEER_MTLS", "value":"on"}),
            json!({"name":"SIFT_PEER_TLS_CERT", "value":"/var/run/secrets/sift/peer-tls/tls.crt"}),
            json!({"name":"SIFT_PEER_TLS_KEY", "value":"/var/run/secrets/sift/peer-tls/tls.key"}),
            json!({"name":"SIFT_PEER_TLS_CA", "value":"/var/run/secrets/sift/peer-tls/ca.crt"}),
        ]);
    }
    if matches!(spec.auth, AuthMode::Kubernetes) {
        env.extend([
            json!({"name":"SIFT_K8S_AUDIENCE", "value":"sift.axiom.dev"}),
            json!({"name":"POD_NAMESPACE", "valueFrom":{"fieldRef":{"fieldPath":"metadata.namespace"}}}),
        ]);
    }
    if matches!(spec.auth, AuthMode::Required) {
        env.push(json!({
            "name":"SIFT_TOKEN_REGISTRY_FILE",
            "value":"/var/run/secrets/sift/token-registry.json"
        }));
    }
    env
}

fn role_support_volumes(spec: &SiftSpec, role: &str) -> (Vec<Value>, Vec<Value>) {
    let mut volumes = Vec::new();
    let mut mounts = Vec::new();
    if matches!(role, STORE_COMPONENT | CONTROL_COMPONENT) {
        volumes.push(json!({
            "name":"peer-tls", "secret":{"secretName":spec.peer_tls_secret}
        }));
        mounts.push(json!({
            "name":"peer-tls", "mountPath":"/var/run/secrets/sift/peer-tls", "readOnly":true
        }));
    }
    if matches!(spec.auth, AuthMode::Required) {
        if let Some(secret) = &spec.tokens_secret {
            volumes.push(json!({"name":"tokens", "secret":{"secretName":secret}}));
            mounts.push(json!({
                "name":"tokens", "mountPath":"/var/run/secrets/sift", "readOnly":true
            }));
        }
    }
    (volumes, mounts)
}

fn role_container_plan(
    cx: &RenderCtx<'_>,
    spec: &SiftSpec,
    role: &str,
    mounts: Vec<Value>,
) -> render::ContainerPlan {
    let mut ports = vec![json!({"name":"http", "containerPort":HTTP_PORT})];
    if matches!(role, GATEWAY_COMPONENT | STORE_COMPONENT) {
        ports.push(json!({"name":"otlp-grpc", "containerPort":OTLP_GRPC_PORT}));
    }
    if matches!(role, STORE_COMPONENT | CONTROL_COMPONENT) {
        ports.push(json!({"name":"raft-mtls", "containerPort":PEER_MTLS_PORT}));
    }
    let mut container = render::ContainerPlan::new(
        "sift",
        &spec.image,
        vec![
            "serve".into(),
            "--role".into(),
            role.into(),
            "--data-dir".into(),
            "/var/lib/sift".into(),
        ],
    );
    container.ports = ports;
    container.env = role_env(cx, spec, role);
    container.volume_mounts = mounts;
    container.security_context = Some(container_security_context());
    container.resources = Some(json!({"requests":{"cpu":"100m","memory":"256Mi"}}));
    container.readiness_probe = Some(
        json!({"httpGet":{"path":"/readyz","port":"http"},"periodSeconds":5,"timeoutSeconds":3,"failureThreshold":60}),
    );
    container.liveness_probe = Some(
        json!({"httpGet":{"path":"/healthz","port":"http"},"periodSeconds":15,"timeoutSeconds":5,"failureThreshold":3}),
    );
    container.startup_probe = Some(
        json!({"httpGet":{"path":"/healthz","port":"http"},"periodSeconds":5,"timeoutSeconds":3,"failureThreshold":120}),
    );
    container
}

fn role_pod_plan(cx: &RenderCtx<'_>, spec: &SiftSpec, role: &str) -> render::PodPlan {
    let (volumes, mounts) = role_support_volumes(spec, role);
    let service_account = if role == STORE_COMPONENT {
        format!("{}-store", cx.name)
    } else {
        cx.name.to_string()
    };
    let mut runtime = render::PodRuntimePolicy::restricted(
        service_account,
        serde_json::to_value(&spec.placement.node_selector).expect("node selector is JSON"),
    )
    .with_automount_service_account_token(matches!(spec.auth, AuthMode::Kubernetes))
    .with_init_containers(vec![data_init_container()])
    .with_volumes(volumes);
    if matches!(role, STORE_COMPONENT | CONTROL_COMPONENT) {
        runtime = runtime.with_affinity(render::dedicated_node_affinity(
            serde_json::to_value(full_role_selector(cx, role)).expect("role selector is JSON"),
        ));
    }
    render::PodPlan::new(role, role_container_plan(cx, spec, role, mounts), runtime)
        .with_selector_labels(role_selector_labels(role))
        .with_labels(role_extra_labels(role))
}

pub(super) fn client_service_plan(cx: &RenderCtx<'_>) -> render::ServicePlan {
    render::ServicePlan::cluster_ip(
        cx.name,
        "frontend",
        role_selector_labels(GATEWAY_COMPONENT),
        vec![
            render::ServicePortPlan::tcp("http", HTTP_PORT, "http"),
            render::ServicePortPlan::tcp("otlp-grpc", OTLP_GRPC_PORT, "otlp-grpc"),
        ],
    )
    .with_selector_component(GATEWAY_COMPONENT)
}

pub(super) fn role_service_plan(cx: &RenderCtx<'_>, role: &str) -> render::ServicePlan {
    let name = role_name(cx, role);
    let mut ports = vec![render::ServicePortPlan::tcp("http", HTTP_PORT, "http")];
    if matches!(role, GATEWAY_COMPONENT | STORE_COMPONENT) {
        ports.push(render::ServicePortPlan::tcp(
            "otlp-grpc",
            OTLP_GRPC_PORT,
            "otlp-grpc",
        ));
    }
    render::ServicePlan::cluster_ip(name, role, role_selector_labels(role), ports)
}

pub(super) fn headless_service_plan(cx: &RenderCtx<'_>, role: &str) -> render::ServicePlan {
    let name = format!("{}-headless", role_name(cx, role));
    render::ServicePlan::headless(
        name,
        role,
        role_selector_labels(role),
        vec![render::ServicePortPlan::tcp(
            "raft-mtls",
            PEER_MTLS_PORT,
            "raft-mtls",
        )],
    )
}

pub(super) fn stateful_role_plan(
    cx: &RenderCtx<'_>,
    spec: &SiftSpec,
    role: &str,
) -> render::StatefulSetPlan {
    let name = role_name(cx, role);
    let headless = format!("{name}-headless");
    render::StatefulSetPlan::new(
        name,
        headless,
        3,
        role_pod_plan(cx, spec, role),
        render::PersistentVolumeClaimPlan::new(
            "data",
            role,
            storage_size(spec, role),
            "/var/lib/sift",
        ),
    )
}

pub(super) fn deployment_role_plan(
    cx: &RenderCtx<'_>,
    spec: &SiftSpec,
    role: &str,
) -> render::DeploymentPlan {
    let name = role_name(cx, role);
    let claim = format!("{name}-data");
    let mut deployment = render::DeploymentPlan::new(name, 1, role_pod_plan(cx, spec, role))
        .with_persistent_claim(render::PersistentVolumeClaimPlan::new(
            claim,
            role,
            storage_size(spec, role),
            "/var/lib/sift",
        ));
    deployment.strategy = Some(json!({"type":"Recreate"}));
    deployment
}

pub(super) fn disruption_budget_plan(
    cx: &RenderCtx<'_>,
    role: &str,
    min_available: u32,
) -> render::PodDisruptionBudgetPlan {
    let name = role_name(cx, role);
    render::PodDisruptionBudgetPlan::min_available(
        name,
        role,
        role_selector_labels(role),
        min_available,
    )
}
