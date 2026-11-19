//! The network policies: DNS, metadata-server and API-server egress, each
//! role's allowed peers, and the FQDN policies for Google APIs.

use std::collections::BTreeMap;

use service_k8s::render::{self, RenderCtx};

use crate::operations::domain::instance_names::{
    AGENT_COMPONENT, ALL_COMPONENT, APP, BACKUP_COMPONENT, CONTROL_COMPONENT, GATEWAY_COMPONENT,
    HTTP_PORT, OTLP_GRPC_PORT, PEER_MTLS_PORT, QUERY_COMPONENT, STORE_COMPONENT,
};
use crate::operations::infrastructure::k8s::role_workloads::{
    full_role_selector, role_selector_labels,
};
use crate::shared_kernel::instance_spec::{
    InstanceAuth as AuthMode, InstanceSettings as SiftSpec, InstanceTopology,
};

fn dns_egress() -> render::NetworkRulePlan {
    render::NetworkRulePlan::new(
        vec![render::NetworkPeerPlan::pods_in_namespace(
            "kube-system",
            BTreeMap::new(),
        )],
        vec![
            render::NetworkPortPlan::udp(53),
            render::NetworkPortPlan::tcp(53),
        ],
    )
}

fn metadata_server_egress() -> render::NetworkRulePlan {
    render::NetworkRulePlan::new(
        vec![render::NetworkPeerPlan::ip_block("169.254.169.254/32")],
        vec![
            render::NetworkPortPlan::tcp(80),
            render::NetworkPortPlan::tcp(8080),
        ],
    )
}

fn kubernetes_api_egress(cidrs: &[String], ports: &[u16]) -> render::NetworkRulePlan {
    render::NetworkRulePlan::new(
        cidrs
            .iter()
            .map(render::NetworkPeerPlan::ip_block)
            .collect(),
        ports
            .iter()
            .copied()
            .map(|port| render::NetworkPortPlan::tcp(i32::from(port)))
            .collect(),
    )
}

fn role_peer(cx: &RenderCtx<'_>, role: &str) -> render::NetworkPeerPlan {
    render::NetworkPeerPlan::same_namespace_pods(full_role_selector(cx, role))
}

fn instance_peer(cx: &RenderCtx<'_>) -> render::NetworkPeerPlan {
    render::NetworkPeerPlan::same_namespace_pods(BTreeMap::from([
        ("app.kubernetes.io/name".to_string(), APP.to_string()),
        (
            "app.kubernetes.io/instance".to_string(),
            cx.name().to_string(),
        ),
    ]))
}

fn role_network_policy(cx: &RenderCtx<'_>, role: &str) -> render::NetworkPolicyPlan {
    render::NetworkPolicyPlan::new(
        format!("{}-{role}-network", cx.name()),
        role,
        role_selector_labels(role),
    )
    .with_egress(dns_egress())
}

fn default_deny_policy(cx: &RenderCtx<'_>) -> render::NetworkPolicyPlan {
    render::NetworkPolicyPlan::new(format!("{}-network", cx.name()), "network", BTreeMap::new())
        .instance_wide()
}

fn store_uses_gcs(spec: &SiftSpec) -> bool {
    spec.archive
        .as_ref()
        .is_some_and(|archive| archive.destination.starts_with("gs://"))
        || spec
            .bootstrap
            .archive_manifest_uri
            .as_deref()
            .is_some_and(|uri| uri.starts_with("gs://"))
}

/// The single-node Pod: the default deny, and one policy for the Pod that
/// accepts the Service ports from anywhere and reaches the metadata server
/// and the API server when they are needed.
pub(super) fn single_node_network_policy_plans(
    cx: &RenderCtx<'_>,
    spec: &SiftSpec,
    kubernetes_api_cidrs: &[String],
    kubernetes_api_ports: &[u16],
) -> Vec<render::NetworkPolicyPlan> {
    let mut all =
        role_network_policy(cx, ALL_COMPONENT).with_ingress(render::NetworkRulePlan::new(
            vec![render::NetworkPeerPlan::any()],
            vec![
                render::NetworkPortPlan::tcp(HTTP_PORT),
                render::NetworkPortPlan::tcp(OTLP_GRPC_PORT),
            ],
        ));
    if store_uses_gcs(spec) {
        all = all.with_egress(metadata_server_egress());
    }
    if matches!(spec.auth, AuthMode::Kubernetes) && !kubernetes_api_cidrs.is_empty() {
        all = all.with_egress(kubernetes_api_egress(
            kubernetes_api_cidrs,
            kubernetes_api_ports,
        ));
    }
    vec![default_deny_policy(cx), all]
}

pub(super) fn network_policy_plans(
    cx: &RenderCtx<'_>,
    spec: &SiftSpec,
    kubernetes_api_cidrs: &[String],
    kubernetes_api_ports: &[u16],
) -> Vec<render::NetworkPolicyPlan> {
    let default_deny = default_deny_policy(cx);

    let mut gateway = role_network_policy(cx, GATEWAY_COMPONENT)
        .with_ingress(render::NetworkRulePlan::new(
            vec![render::NetworkPeerPlan::any()],
            vec![
                render::NetworkPortPlan::tcp(HTTP_PORT),
                render::NetworkPortPlan::tcp(OTLP_GRPC_PORT),
            ],
        ))
        .with_egress(render::NetworkRulePlan::new(
            vec![role_peer(cx, QUERY_COMPONENT)],
            vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
        ))
        .with_egress(render::NetworkRulePlan::new(
            vec![role_peer(cx, STORE_COMPONENT)],
            vec![
                render::NetworkPortPlan::tcp(HTTP_PORT),
                render::NetworkPortPlan::tcp(OTLP_GRPC_PORT),
            ],
        ));
    let mut query = role_network_policy(cx, QUERY_COMPONENT)
        .with_ingress(render::NetworkRulePlan::new(
            vec![role_peer(cx, GATEWAY_COMPONENT)],
            vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
        ))
        .with_egress(render::NetworkRulePlan::new(
            vec![role_peer(cx, STORE_COMPONENT)],
            vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
        ));
    let mut store = role_network_policy(cx, STORE_COMPONENT)
        .with_ingress(render::NetworkRulePlan::new(
            vec![role_peer(cx, GATEWAY_COMPONENT)],
            vec![
                render::NetworkPortPlan::tcp(HTTP_PORT),
                render::NetworkPortPlan::tcp(OTLP_GRPC_PORT),
            ],
        ))
        .with_ingress(render::NetworkRulePlan::new(
            vec![role_peer(cx, QUERY_COMPONENT)],
            vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
        ))
        .with_ingress(render::NetworkRulePlan::new(
            vec![role_peer(cx, STORE_COMPONENT)],
            vec![render::NetworkPortPlan::tcp(PEER_MTLS_PORT)],
        ))
        .with_egress(render::NetworkRulePlan::new(
            vec![role_peer(cx, STORE_COMPONENT)],
            vec![render::NetworkPortPlan::tcp(PEER_MTLS_PORT)],
        ));
    let mut control = role_network_policy(cx, CONTROL_COMPONENT)
        .with_ingress(render::NetworkRulePlan::new(
            vec![instance_peer(cx)],
            vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
        ))
        .with_ingress(render::NetworkRulePlan::new(
            vec![role_peer(cx, CONTROL_COMPONENT)],
            vec![render::NetworkPortPlan::tcp(PEER_MTLS_PORT)],
        ))
        .with_egress(render::NetworkRulePlan::new(
            vec![role_peer(cx, CONTROL_COMPONENT)],
            vec![render::NetworkPortPlan::tcp(PEER_MTLS_PORT)],
        ));
    gateway = gateway.with_egress(render::NetworkRulePlan::new(
        vec![role_peer(cx, CONTROL_COMPONENT)],
        vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
    ));
    query = query.with_ingress(render::NetworkRulePlan::new(
        vec![role_peer(cx, CONTROL_COMPONENT)],
        vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
    ));
    control = control.with_egress(render::NetworkRulePlan::new(
        vec![role_peer(cx, QUERY_COMPONENT)],
        vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
    ));
    control = control.with_egress(render::NetworkRulePlan::new(
        vec![role_peer(cx, STORE_COMPONENT)],
        vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
    ));
    store = store.with_ingress(render::NetworkRulePlan::new(
        vec![role_peer(cx, CONTROL_COMPONENT)],
        vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
    ));
    let mut agent =
        role_network_policy(cx, AGENT_COMPONENT).with_egress(render::NetworkRulePlan::new(
            vec![role_peer(cx, GATEWAY_COMPONENT)],
            vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
        ));
    agent = agent.with_egress(render::NetworkRulePlan::new(
        vec![render::NetworkPeerPlan::same_namespace_pods(
            BTreeMap::from([("sift.axiom.dev/scrape".into(), "true".into())]),
        )],
        vec![],
    ));
    agent = agent.with_ingress(render::NetworkRulePlan::new(
        vec![render::NetworkPeerPlan::same_namespace_pods(
            BTreeMap::from([
                ("app.kubernetes.io/name".into(), APP.into()),
                ("app.kubernetes.io/instance".into(), cx.name().into()),
                ("sift.axiom.dev/role".into(), AGENT_COMPONENT.into()),
            ]),
        )],
        vec![render::NetworkPortPlan::tcp(9318)],
    ));
    if !kubernetes_api_cidrs.is_empty() {
        agent = agent.with_egress(kubernetes_api_egress(
            kubernetes_api_cidrs,
            kubernetes_api_ports,
        ));
    }
    let mut backup =
        role_network_policy(cx, BACKUP_COMPONENT).with_egress(render::NetworkRulePlan::new(
            vec![role_peer(cx, GATEWAY_COMPONENT)],
            vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
        ));
    if matches!(spec.auth, AuthMode::Kubernetes) && !kubernetes_api_cidrs.is_empty() {
        gateway = gateway.with_egress(kubernetes_api_egress(
            kubernetes_api_cidrs,
            kubernetes_api_ports,
        ));
        query = query.with_egress(kubernetes_api_egress(
            kubernetes_api_cidrs,
            kubernetes_api_ports,
        ));
        store = store.with_egress(kubernetes_api_egress(
            kubernetes_api_cidrs,
            kubernetes_api_ports,
        ));
        control = control.with_egress(kubernetes_api_egress(
            kubernetes_api_cidrs,
            kubernetes_api_ports,
        ));
    }

    if store_uses_gcs(spec) {
        store = store.with_egress(metadata_server_egress());
        control = control.with_egress(metadata_server_egress());
    }
    if spec
        .backup
        .as_ref()
        .is_some_and(|backup| backup.destination.starts_with("gs://"))
    {
        backup = backup.with_egress(metadata_server_egress());
    }

    vec![default_deny, gateway, query, store, control, agent, backup]
}

pub(super) fn fqdn_network_policy_plans(
    cx: &RenderCtx<'_>,
    spec: &SiftSpec,
) -> Vec<render::FqdnNetworkPolicyPlan> {
    let mut policies = Vec::new();
    if spec.topology == InstanceTopology::SingleNode {
        // A single-node instance has no control Pod, so a webhook or a backup
        // policy has nothing to apply to. Only the GCS archive needs a policy.
        if store_uses_gcs(spec) {
            policies.push(google_apis_policy(cx, ALL_COMPONENT));
        }
        return policies;
    }
    if !spec.alerting.allowed_hosts.is_empty() {
        let mut policy = render::FqdnNetworkPolicyPlan::new(
            format!("{}-control-webhooks", cx.name()),
            CONTROL_COMPONENT,
            full_role_selector(cx, CONTROL_COMPONENT),
        )
        .with_port(render::NetworkPortPlan::tcp(443));
        for host in &spec.alerting.allowed_hosts {
            policy = policy.with_match(render::FqdnMatchPlan::name(host));
        }
        policies.push(policy);
    }
    if store_uses_gcs(spec) {
        policies.push(google_apis_policy(cx, STORE_COMPONENT));
        policies.push(google_apis_policy(cx, CONTROL_COMPONENT));
    }
    if spec
        .backup
        .as_ref()
        .is_some_and(|backup| backup.destination.starts_with("gs://"))
    {
        policies.push(google_apis_policy(cx, BACKUP_COMPONENT));
    }
    policies
}

fn google_apis_policy(cx: &RenderCtx<'_>, role: &str) -> render::FqdnNetworkPolicyPlan {
    render::FqdnNetworkPolicyPlan::new(
        format!("{}-{role}-google-apis", cx.name()),
        role,
        full_role_selector(cx, role),
    )
    .with_match(render::FqdnMatchPlan::name("storage.googleapis.com"))
    .with_port(render::NetworkPortPlan::tcp(443))
}
