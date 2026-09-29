//! The network policies: DNS, metadata-server and API-server egress, each
//! role's allowed peers, and the FQDN policies for Google APIs.

use std::collections::BTreeMap;

use service_k8s::render::{self, RenderCtx};

use crate::operations::infrastructure::k8s::constants::{
    AGENT_COMPONENT, APP, BACKUP_COMPONENT, CONTROL_COMPONENT, GATEWAY_COMPONENT, HTTP_PORT,
    OTLP_GRPC_PORT, PEER_MTLS_PORT, QUERY_COMPONENT, STORE_COMPONENT,
};
use crate::operations::infrastructure::k8s::role_workloads::{
    full_role_selector, role_selector_labels,
};
use crate::operations::interfaces::crd::sift_spec::{AuthMode, SiftSpec};

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
            .map(|cidr| render::NetworkPeerPlan::ip_block(cidr))
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
            cx.name.to_string(),
        ),
    ]))
}

fn role_network_policy(cx: &RenderCtx<'_>, role: &str) -> render::NetworkPolicyPlan {
    render::NetworkPolicyPlan::new(
        format!("{}-{role}-network", cx.name),
        role,
        role_selector_labels(role),
    )
    .with_egress(dns_egress())
}

pub(super) fn network_policy_plans(
    cx: &RenderCtx<'_>,
    spec: &SiftSpec,
    kubernetes_api_cidrs: &[String],
    kubernetes_api_ports: &[u16],
) -> Vec<render::NetworkPolicyPlan> {
    let default_deny =
        render::NetworkPolicyPlan::new(format!("{}-network", cx.name), "network", BTreeMap::new())
            .instance_wide();

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
    let agent = role_network_policy(cx, AGENT_COMPONENT).with_egress(render::NetworkRulePlan::new(
        vec![role_peer(cx, GATEWAY_COMPONENT)],
        vec![render::NetworkPortPlan::tcp(HTTP_PORT)],
    ));
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

    let store_uses_gcs = spec
        .archive
        .as_ref()
        .is_some_and(|archive| archive.destination.starts_with("gs://"))
        || spec
            .bootstrap
            .archive_manifest_uri
            .as_deref()
            .is_some_and(|uri| uri.starts_with("gs://"));
    if store_uses_gcs {
        store = store.with_egress(metadata_server_egress());
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
    let store_uses_gcs = spec
        .archive
        .as_ref()
        .is_some_and(|archive| archive.destination.starts_with("gs://"))
        || spec
            .bootstrap
            .archive_manifest_uri
            .as_deref()
            .is_some_and(|uri| uri.starts_with("gs://"));
    if store_uses_gcs {
        policies.push(google_apis_policy(cx, STORE_COMPONENT));
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
        format!("{}-{role}-google-apis", cx.name),
        role,
        full_role_selector(cx, role),
    )
    .with_match(render::FqdnMatchPlan::name("storage.googleapis.com"))
    .with_port(render::NetworkPortPlan::tcp(443))
}
