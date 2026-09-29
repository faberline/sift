//! The managed-service adapter: how service_k8s renders a Sift resource's
//! children, reads its readiness, and reports its status.

use std::collections::BTreeMap;

use kube::ResourceExt;
use serde_json::{json, Value};
use service_k8s::render::{self, RenderCtx};
use service_k8s::{ManagedService, ReadinessTarget, ReadyFacts};

use crate::operations::infrastructure::k8s::agent_workloads::{
    agent_daemon_set_plan, agent_project_role_binding_plan, agent_project_role_plan,
};
use crate::operations::infrastructure::k8s::api_endpoint_discovery::discover_kubernetes_api_endpoint;
use crate::operations::infrastructure::k8s::backup_cron_job::backup_cron_job_plan;
use crate::operations::infrastructure::k8s::constants::{
    API_VERSION, APP, AUTH_DELEGATION_COMPONENT, AUTH_DELEGATOR_ROLE, BACKUP_COMPONENT,
    CONTROL_COMPONENT, GATEWAY_COMPONENT, KIND, QUERY_COMPONENT, STORE_COMPONENT,
};
use crate::operations::infrastructure::k8s::network_policy::{
    fqdn_network_policy_plans, network_policy_plans,
};
use crate::operations::infrastructure::k8s::role_workloads::{
    auth_delegator_binding_name, client_service_plan, deployment_role_plan, disruption_budget_plan,
    headless_service_plan, role_service_plan, stateful_role_plan,
};
use crate::operations::interfaces::crd::sift_spec::{AuthMode, Sift};

fn render_children(
    sift: &Sift,
    kubernetes_api_cidrs: &[String],
    kubernetes_api_ports: &[u16],
) -> Vec<Value> {
    let name = sift.name_any();
    let namespace = sift.namespace().unwrap_or_else(|| "default".to_string());
    let owner = sift
        .metadata
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

    if matches!(sift.spec.auth, AuthMode::Kubernetes) {
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
        if let Some(uid) = sift.metadata.uid.as_deref() {
            binding = binding.with_label("service-k8s.axiom.dev/owner-uid", uid);
        }
        plan.add_cluster_role_binding(binding);
    }

    plan.add_service(client_service_plan(&cx));
    for role in [STORE_COMPONENT, CONTROL_COMPONENT] {
        plan.add_service(role_service_plan(&cx, role));
        plan.add_service(headless_service_plan(&cx, role));
        plan.add_stateful_set(stateful_role_plan(&cx, &sift.spec, role));
        plan.add_pod_disruption_budget(disruption_budget_plan(&cx, role, 2));
    }
    for role in [GATEWAY_COMPONENT, QUERY_COMPONENT] {
        plan.add_service(role_service_plan(&cx, role));
        plan.add_deployment(deployment_role_plan(&cx, &sift.spec, role));
    }
    plan.add_daemon_set(agent_daemon_set_plan(&cx, &sift.spec));
    for policy in network_policy_plans(&cx, &sift.spec, kubernetes_api_cidrs, kubernetes_api_ports)
    {
        plan.add_network_policy(policy);
    }
    for policy in fqdn_network_policy_plans(&cx, &sift.spec) {
        plan.add_fqdn_network_policy(policy);
    }

    if let Some(backup) = &sift.spec.backup {
        plan.add_cron_job(backup_cron_job_plan(&cx, &sift.spec, backup));
    }
    plan.render()
        .expect("Sift typed Kubernetes workload plan must be valid")
}

impl ManagedService for Sift {
    const MANAGER: &'static str = "sift-operator";

    fn render(&self) -> Vec<Value> {
        render_children(self, &[], &[])
    }

    fn reconcile_plan(
        &self,
        client: kube::Client,
    ) -> impl std::future::Future<Output = anyhow::Result<service_k8s::service::ReconcilePlan>> + Send
    {
        let sift = self.clone();
        async move {
            let (api_cidrs, api_ports) = if matches!(sift.spec.auth, AuthMode::Kubernetes) {
                discover_kubernetes_api_endpoint(client).await?
            } else {
                (Vec::new(), Vec::new())
            };
            Ok(service_k8s::service::ReconcilePlan {
                children: render_children(&sift, &api_cidrs, &api_ports),
                context: Value::Null,
            })
        }
    }

    fn readiness_targets(&self) -> Vec<ReadinessTarget> {
        let name = self.name_any();
        vec![
            ReadinessTarget {
                kind: "StatefulSet",
                name: format!("{name}-store"),
            },
            ReadinessTarget {
                kind: "StatefulSet",
                name: format!("{name}-control"),
            },
            ReadinessTarget {
                kind: "Deployment",
                name: format!("{name}-gateway"),
            },
            ReadinessTarget {
                kind: "Deployment",
                name: format!("{name}-query"),
            },
            ReadinessTarget {
                kind: "DaemonSet",
                name: format!("{name}-agent"),
            },
        ]
    }

    fn status_patch(&self, ready: &ReadyFacts) -> Value {
        let name = self.name_any();
        let store_ready = ready.get(&format!("{name}-store")).max(0);
        let control_ready = ready.get(&format!("{name}-control")).max(0);
        let gateway_ready = ready.get(&format!("{name}-gateway")).max(0);
        let query_ready = ready.get(&format!("{name}-query")).max(0);
        let agent_ready = ready.get(&format!("{name}-agent")).max(0);
        let ready_replicas =
            store_ready + control_ready + gateway_ready + query_ready + agent_ready;
        let desired_replicas_per_shard = self.spec.replicas_per_shard;
        let supported_topology = desired_replicas_per_shard == 3 && self.spec.voter_count == 3;
        let (phase, message) = if !supported_topology {
            (
                "UnsupportedTopology",
                format!(
                    "requested 1 shard x {desired_replicas_per_shard} replicas with {} voters; Sift requires exactly three durable voters",
                    self.spec.voter_count
                ),
            )
        } else if store_ready >= 3
            && control_ready >= 3
            && gateway_ready >= 1
            && query_ready >= 1
            && agent_ready >= 1
        {
            (
                "Ready",
                format!(
                    "store {store_ready}/3, control {control_ready}/3, gateway {gateway_ready}/1, query {query_ready}/1, agent {agent_ready} ready"
                ),
            )
        } else {
            (
                "Pending",
                format!(
                    "store {store_ready}/3, control {control_ready}/3, gateway {gateway_ready}/1, query {query_ready}/1, agent {agent_ready} ready"
                ),
            )
        };
        let (backup_phase, backup_message) = if self.spec.backup.is_some() {
            (
                "Configured",
                "scheduled live backup is configured; execution evidence is reported by its CronJob and destination",
            )
        } else {
            ("NotConfigured", "no scheduled backup requested")
        };
        let archive_phase = if self.spec.archive.is_some() {
            "Configured"
        } else {
            "NotConfigured"
        };
        let (restore_phase, restore_source_manifest) = self
            .spec
            .bootstrap
            .archive_manifest_uri
            .as_deref()
            .map(|uri| {
                let restore_phase = if store_ready >= 3 {
                    "Restored"
                } else if store_ready > 0 {
                    "Restoring"
                } else {
                    "Requested"
                };
                (restore_phase, uri)
            })
            .unwrap_or(("NotRequested", ""));
        json!({
            "status": {
                "phase": phase,
                "observedGeneration": self.metadata.generation.unwrap_or(0),
                "readyReplicas": ready_replicas,
                "desiredShardCount": 1,
                "currentShardCount": u32::from(store_ready >= 2),
                "desiredReplicasPerShard": desired_replicas_per_shard,
                "currentReadyReplicasPerShard": store_ready.min(3) as u32,
                "backupPhase": backup_phase,
                "backupMessage": backup_message,
                "archivePhase": archive_phase,
                "archiveWatermark": 0,
                "lastArchiveManifest": "",
                "restorePhase": restore_phase,
                "restoreSourceManifest": restore_source_manifest,
                "backpressure": "Healthy",
                "lastDataError": "",
                "message": message,
            }
        })
    }

    fn prunes(&self) -> Vec<service_k8s::service::PruneTarget> {
        let name = self.name_any();
        let store_uses_gcs = self
            .spec
            .archive
            .as_ref()
            .is_some_and(|archive| archive.destination.starts_with("gs://"))
            || self
                .spec
                .bootstrap
                .archive_manifest_uri
                .as_deref()
                .is_some_and(|uri| uri.starts_with("gs://"));
        let backup_uses_gcs = self
            .spec
            .backup
            .as_ref()
            .is_some_and(|backup| backup.destination.starts_with("gs://"));
        [
            (STORE_COMPONENT, store_uses_gcs),
            (BACKUP_COMPONENT, backup_uses_gcs),
        ]
        .into_iter()
        .filter(|(_, desired)| !desired)
        .map(|(role, _)| service_k8s::service::PruneTarget {
            api_version: "networking.gke.io/v1alpha1",
            kind: "FQDNNetworkPolicy",
            name: format!("{name}-{role}-google-apis"),
        })
        .collect()
    }

    fn cluster_scoped_children(&self) -> Vec<service_k8s::service::ClusterScopedChild> {
        let Some(uid) = self.metadata.uid.as_deref() else {
            return Vec::new();
        };
        let name = self.name_any();
        let namespace = self.namespace().unwrap_or_else(|| "default".to_string());
        vec![service_k8s::service::ClusterScopedChild {
            api_version: "rbac.authorization.k8s.io/v1",
            kind: "ClusterRoleBinding",
            name: format!("sift.{namespace}.{name}.auth-delegator"),
            expected_labels: BTreeMap::from([
                ("app.kubernetes.io/name".to_string(), APP.to_string()),
                ("app.kubernetes.io/instance".to_string(), name),
                (
                    "app.kubernetes.io/component".to_string(),
                    AUTH_DELEGATION_COMPONENT.to_string(),
                ),
                ("sift.axiom.dev/owner-namespace".to_string(), namespace),
                (
                    "service-k8s.axiom.dev/owner-uid".to_string(),
                    uid.to_string(),
                ),
            ]),
            desired: matches!(self.spec.auth, AuthMode::Kubernetes),
        }]
    }
}
