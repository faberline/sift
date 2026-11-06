//! The managed-service adapter: how service_k8s renders a Sift resource's
//! children, reads its readiness, and reports its status.

use serde_json::Value;
use service_k8s::render::{self, RenderCtx};

use crate::operations::domain::instance_names::{
    API_VERSION, APP, AUTH_DELEGATION_COMPONENT, AUTH_DELEGATOR_ROLE, BACKUP_COMPONENT,
    CONTROL_COMPONENT, GATEWAY_COMPONENT, KIND, QUERY_COMPONENT, STORE_COMPONENT,
};
use crate::operations::domain::instance_renderer::InstanceRenderer;
use crate::operations::infrastructure::k8s::agent_workloads::{
    agent_daemon_set_plan, agent_project_role_binding_plan, agent_project_role_plan,
};
use crate::operations::infrastructure::k8s::api_endpoint_discovery::discover_kubernetes_api_endpoint;
use crate::operations::infrastructure::k8s::backup_cron_job::backup_cron_job_plan;
use crate::operations::infrastructure::k8s::network_policy::{
    fqdn_network_policy_plans, network_policy_plans,
};
use crate::operations::infrastructure::k8s::role_workloads::{
    auth_delegator_binding_name, client_service_plan, deployment_role_plan, disruption_budget_plan,
    headless_service_plan, role_service_plan, stateful_role_plan,
};
use crate::shared_kernel::instance_spec::{InstanceAuth as AuthMode, InstanceSpec};

fn render_children(
    instance: &InstanceSpec,
    kubernetes_api_cidrs: &[String],
    kubernetes_api_ports: &[u16],
) -> Vec<Value> {
    let name = instance.name.clone();
    let namespace = instance.namespace.clone();
    let owner = instance
        .uid
        .as_deref()
        .map(|uid| render::owner_ref(API_VERSION, KIND, &name, uid));
    let cx = RenderCtx {
        app: APP,
        manager: "sift-operator",
        api_version: API_VERSION,
        kind: KIND,
        name: &name,
        ns: &namespace,
        owner,
    };
    let mut plan = render::WorkloadPlan::new(&cx);
    plan.add_service_account(render::ServiceAccountPlan::new(&name, "runtime"));
    plan.add_service_account(render::ServiceAccountPlan::new(
        format!("{name}-store"),
        STORE_COMPONENT,
    ));
    plan.add_service_account(render::ServiceAccountPlan::new(
        format!("{name}-backup"),
        BACKUP_COMPONENT,
    ));

    if matches!(instance.settings.auth, AuthMode::Kubernetes) {
        plan.add_role(agent_project_role_plan(&cx));
        plan.add_role_binding(agent_project_role_binding_plan(&cx));
        let mut binding = render::ClusterRoleBindingPlan::new(
            auth_delegator_binding_name(&cx),
            AUTH_DELEGATION_COMPONENT,
            AUTH_DELEGATOR_ROLE,
        )
        .with_service_account(render::ServiceAccountSubjectPlan::new(&namespace, &name))
        .with_service_account(render::ServiceAccountSubjectPlan::new(
            &namespace,
            format!("{name}-store"),
        ))
        .with_label("sift.axiom.dev/owner-namespace", &namespace);
        if let Some(uid) = instance.uid.as_deref() {
            binding = binding.with_label("service-k8s.axiom.dev/owner-uid", uid);
        }
        plan.add_cluster_role_binding(binding);
    }

    plan.add_service(client_service_plan(&cx));
    for role in [STORE_COMPONENT, CONTROL_COMPONENT] {
        plan.add_service(role_service_plan(&cx, role));
        plan.add_service(headless_service_plan(&cx, role));
        plan.add_stateful_set(stateful_role_plan(&cx, &instance.settings, role));
        plan.add_pod_disruption_budget(disruption_budget_plan(&cx, role, 2));
    }
    for role in [GATEWAY_COMPONENT, QUERY_COMPONENT] {
        plan.add_service(role_service_plan(&cx, role));
        plan.add_deployment(deployment_role_plan(&cx, &instance.settings, role));
    }
    plan.add_daemon_set(agent_daemon_set_plan(&cx, &instance.settings));
    for policy in network_policy_plans(
        &cx,
        &instance.settings,
        kubernetes_api_cidrs,
        kubernetes_api_ports,
    ) {
        plan.add_network_policy(policy);
    }
    for policy in fqdn_network_policy_plans(&cx, &instance.settings) {
        plan.add_fqdn_network_policy(policy);
    }

    if let Some(backup) = &instance.settings.backup {
        plan.add_cron_job(backup_cron_job_plan(&cx, &instance.settings, backup));
    }
    plan.render()
        .expect("Sift typed Kubernetes workload plan must be valid")
}

pub(crate) struct KubernetesRenderer;

impl InstanceRenderer for KubernetesRenderer {
    type Client = kube::Client;
    type Error = anyhow::Error;

    fn render(
        &self,
        instance: &InstanceSpec,
        api_cidrs: &[String],
        api_ports: &[u16],
    ) -> Vec<Value> {
        render_children(instance, api_cidrs, api_ports)
    }

    fn discover_api_endpoint(
        &self,
        client: kube::Client,
    ) -> crate::shared_kernel::port_future::PortFuture<'_, (Vec<String>, Vec<u16>), anyhow::Error>
    {
        Box::pin(discover_kubernetes_api_endpoint(client))
    }
}
