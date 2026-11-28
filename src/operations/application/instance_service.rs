//! Render workloads and report instance state through the platform port.

use crate::operations::domain::instance_names::{
    ALL_COMPONENT, APP, AUTH_DELEGATION_COMPONENT, BACKUP_COMPONENT, CONTROL_COMPONENT,
    STORE_COMPONENT,
};
use crate::operations::domain::instance_renderer::InstanceRenderer;
use crate::shared_kernel::instance_spec::{
    InstanceAuth as AuthMode, InstanceSpec, InstanceTopology,
};
use serde_json::{json, Value};
use service_k8s::{ReadinessTarget, ReadyFacts};
use std::collections::BTreeMap;
use std::sync::Arc;

/// The operator environment variable that says the auth-delegator
/// `ClusterRoleBinding` is created by hand, so the operator must not render it.
pub(crate) const PRECREATED_AUTH_DELEGATOR_ENV: &str = "SIFT_OPERATOR_PRECREATED_AUTH_DELEGATOR";

/// True only for the exact value `true`. Any other value keeps the default.
pub(crate) fn precreated_auth_delegator_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// The name of the auth-delegator `ClusterRoleBinding` of `instance`.
fn auth_delegator_binding_name(instance: &InstanceSpec) -> String {
    format!(
        "sift.{}.{}.auth-delegator",
        instance.namespace, instance.name
    )
}

pub(crate) struct InstanceService {
    renderer: Arc<dyn InstanceRenderer<Client = kube::Client, Error = anyhow::Error>>,
    precreated_auth_delegator: bool,
}

impl InstanceService {
    pub(crate) fn new(
        renderer: Arc<dyn InstanceRenderer<Client = kube::Client, Error = anyhow::Error>>,
    ) -> Self {
        Self {
            renderer,
            precreated_auth_delegator: false,
        }
    }

    pub(crate) fn with_precreated_auth_delegator(mut self, on: bool) -> Self {
        self.precreated_auth_delegator = on;
        self
    }

    /// Removes the auth-delegator binding of `instance` from `children` when
    /// the binding is precreated.
    fn without_auth_delegator(&self, instance: &InstanceSpec, children: Vec<Value>) -> Vec<Value> {
        if !self.precreated_auth_delegator {
            return children;
        }
        let binding_name = auth_delegator_binding_name(instance);
        children
            .into_iter()
            .filter(|child| {
                !(child["kind"] == "ClusterRoleBinding"
                    && child["metadata"]["name"] == binding_name)
            })
            .collect()
    }

    pub(crate) fn render(&self, instance: &InstanceSpec) -> Vec<Value> {
        let children = self.renderer.render(instance, &[], &[]);
        self.without_auth_delegator(instance, children)
    }

    pub(crate) async fn reconcile_plan(
        &self,
        instance: &InstanceSpec,
        client: kube::Client,
    ) -> anyhow::Result<service_k8s::service::ReconcilePlan> {
        // The metrics agent lists Kubernetes Pods in every auth mode.
        let (api_cidrs, api_ports) = self.renderer.discover_api_endpoint(client.clone()).await?;
        let children = self.renderer.render(instance, &api_cidrs, &api_ports);
        let children = self.without_auth_delegator(instance, children);
        // Read the pre-created binding only when the operator is not creating it.
        let context = if self.precreated_auth_delegator
            && matches!(instance.settings.auth, AuthMode::Kubernetes)
        {
            let api = kube::Api::<k8s_openapi::api::rbac::v1::ClusterRoleBinding>::all(
                client.clone(),
            );
            let found = api
                .get_opt(&auth_delegator_binding_name(instance))
                .await?
                .is_some();
            json!({ "authDelegatorBindingFound": found })
        } else {
            Value::Null
        };
        // Only a GCS destination or an alerting host list renders an FQDN policy.
        if children
            .iter()
            .any(|child| child["kind"] == "FQDNNetworkPolicy")
        {
            self.renderer.ensure_fqdn_support(client).await?;
        }
        Ok(service_k8s::service::ReconcilePlan::new(children, context))
    }

    /// Builds the `AuthDelegatorBinding` fact from the plan context. It does
    /// no I/O: `reconcile_plan` reads the binding and puts the result in `context`.
    /// When the mode is switched off later, `service-k8s` keeps the old
    /// condition as it is, because a service that declares no facts keeps the
    /// earlier conditions.
    pub(crate) fn conditions(
        &self,
        instance: &InstanceSpec,
        context: &Value,
    ) -> Vec<service_k8s::ConditionFact> {
        use service_k8s::{ConditionFact, ConditionStatus};
        let name = auth_delegator_binding_name(instance);
        match context["authDelegatorBindingFound"].as_bool() {
            Some(false) => vec![ConditionFact::new(
                "AuthDelegatorBinding",
                ConditionStatus::False,
                "PrecreatedBindingMissing",
                format!(
                    "ClusterRoleBinding {name} (role system:auth-delegator) does not exist; the cluster admin creates it before the Sift is ready"
                ),
            )],
            Some(true) => vec![ConditionFact::new(
                "AuthDelegatorBinding",
                ConditionStatus::True,
                "PrecreatedBindingFound",
                format!("ClusterRoleBinding {name} exists"),
            )],
            None => Vec::new(),
        }
    }

    pub(crate) fn readiness_targets(&self, instance: &InstanceSpec) -> Vec<ReadinessTarget> {
        let name = instance.name.clone();
        if instance.settings.topology == InstanceTopology::SingleNode {
            return vec![ReadinessTarget::new("StatefulSet", format!("{name}-all"))];
        }
        let mut targets = vec![
            ReadinessTarget::new("StatefulSet", format!("{name}-store")),
            ReadinessTarget::new("StatefulSet", format!("{name}-control")),
            ReadinessTarget::new("Deployment", format!("{name}-gateway")),
            ReadinessTarget::new("Deployment", format!("{name}-query")),
        ];
        if instance.settings.logs.agent_enabled {
            targets.push(ReadinessTarget::new("DaemonSet", format!("{name}-agent")));
        }
        targets
    }

    pub(crate) fn status_patch(&self, instance: &InstanceSpec, ready: &ReadyFacts) -> Value {
        let name = instance.name.clone();
        let store_ready = ready.get(&format!("{name}-store")).max(0);
        let control_ready = ready.get(&format!("{name}-control")).max(0);
        let gateway_ready = ready.get(&format!("{name}-gateway")).max(0);
        let query_ready = ready.get(&format!("{name}-query")).max(0);
        let agent_enabled = instance.settings.logs.agent_enabled;
        // A disabled agent has no replicas, so its ready count never counts.
        let agent_ready = if agent_enabled {
            ready.get(&format!("{name}-agent")).max(0)
        } else {
            0
        };
        let agent_text = if agent_enabled {
            format!("agent {agent_ready} ready")
        } else {
            "agent disabled".to_string()
        };
        let single_node = instance.settings.topology == InstanceTopology::SingleNode;
        let all_ready = ready.get(&format!("{name}-all")).max(0);
        let ready_replicas = if single_node {
            all_ready
        } else {
            store_ready + control_ready + gateway_ready + query_ready + agent_ready
        };
        let desired_replicas_per_shard = if single_node {
            1
        } else {
            instance.settings.replicas_per_shard
        };
        let supported_topology =
            desired_replicas_per_shard == 3 && instance.settings.voter_count == 3;
        let (current_shards, current_replicas_per_shard) = if single_node {
            (u32::from(all_ready >= 1), all_ready.min(1) as u32)
        } else {
            (u32::from(store_ready >= 2), store_ready.min(3) as u32)
        };
        let (phase, message) = if single_node {
            (
                if all_ready >= 1 { "Ready" } else { "Pending" },
                format!("all {all_ready}/1 ready"),
            )
        } else if !supported_topology {
            (
                "UnsupportedTopology",
                format!(
                    "requested 1 shard x {desired_replicas_per_shard} replicas with {} voters; Sift requires exactly three durable voters",
                    instance.settings.voter_count
                ),
            )
        } else if store_ready >= 3
            && control_ready >= 3
            && gateway_ready >= 1
            && query_ready >= 1
            && (!agent_enabled || agent_ready >= 1)
        {
            (
                "Ready",
                format!(
                    "store {store_ready}/3, control {control_ready}/3, gateway {gateway_ready}/1, query {query_ready}/1, {agent_text}"
                ),
            )
        } else {
            (
                "Pending",
                format!(
                    "store {store_ready}/3, control {control_ready}/3, gateway {gateway_ready}/1, query {query_ready}/1, {agent_text}"
                ),
            )
        };
        let (backup_phase, backup_message) = if instance.settings.backup.is_some() {
            (
                "Configured",
                "scheduled live backup is configured; execution evidence is reported by its CronJob and destination",
            )
        } else {
            ("NotConfigured", "no scheduled backup requested")
        };
        let archive_phase = if instance.settings.archive.is_some() {
            "Configured"
        } else {
            "NotConfigured"
        };
        let (restore_phase, restore_source_manifest) = instance
            .settings
            .bootstrap
            .archive_manifest_uri
            .as_deref()
            .map(|uri| {
                let restore_phase = if single_node {
                    if all_ready >= 1 {
                        "Restored"
                    } else {
                        "Requested"
                    }
                } else if store_ready >= 3 {
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
                "observedGeneration": instance.generation,
                "readyReplicas": ready_replicas,
                "desiredShardCount": 1,
                "currentShardCount": current_shards,
                "desiredReplicasPerShard": desired_replicas_per_shard,
                "currentReadyReplicasPerShard": current_replicas_per_shard,
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

    pub(crate) fn prunes(&self, instance: &InstanceSpec) -> Vec<service_k8s::service::PruneTarget> {
        let name = instance.name.clone();
        let store_uses_gcs = instance
            .settings
            .archive
            .as_ref()
            .is_some_and(|archive| archive.destination.starts_with("gs://"))
            || instance
                .settings
                .bootstrap
                .archive_manifest_uri
                .as_deref()
                .is_some_and(|uri| uri.starts_with("gs://"));
        let backup_uses_gcs = instance
            .settings
            .backup
            .as_ref()
            .is_some_and(|backup| backup.destination.starts_with("gs://"));
        // Each entry is (role, wanted). A single-node instance has no store,
        // control or backup Pod, so their policies go; its one Pod keeps the
        // policy only while the archive is a `gs://` destination.
        let roles = if instance.settings.topology == InstanceTopology::SingleNode {
            vec![
                (STORE_COMPONENT, false),
                (CONTROL_COMPONENT, false),
                (BACKUP_COMPONENT, false),
                (ALL_COMPONENT, store_uses_gcs),
            ]
        } else {
            vec![
                (STORE_COMPONENT, store_uses_gcs),
                (CONTROL_COMPONENT, store_uses_gcs),
                (BACKUP_COMPONENT, backup_uses_gcs),
            ]
        };
        roles
            .into_iter()
            .filter(|(_, desired)| !desired)
            .map(|(role, _)| {
                service_k8s::service::PruneTarget::new(
                    "networking.gke.io/v1alpha1",
                    "FQDNNetworkPolicy",
                    format!("{name}-{role}-google-apis"),
                )
            })
            .collect()
    }

    // With the precreated mode the operator lists no cluster-scoped child. A
    // Sift object that the operator made before the mode was switched on keeps
    // its old cluster-children finalizer and its old binding; the admin removes both.
    pub(crate) fn cluster_scoped_children(
        &self,
        instance: &InstanceSpec,
    ) -> Vec<service_k8s::service::ClusterScopedChild> {
        if self.precreated_auth_delegator {
            return Vec::new();
        }
        let Some(uid) = instance.uid.as_deref() else {
            return Vec::new();
        };
        let name = instance.name.clone();
        let namespace = instance.namespace.clone();
        vec![service_k8s::service::ClusterScopedChild::new(
            "rbac.authorization.k8s.io/v1",
            "ClusterRoleBinding",
            format!("sift.{namespace}.{name}.auth-delegator"),
            matches!(instance.settings.auth, AuthMode::Kubernetes),
        )
        .with_expected_labels(BTreeMap::from([
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
        ]))]
    }
}
