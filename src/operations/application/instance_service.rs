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

pub(crate) struct InstanceService {
    renderer: Arc<dyn InstanceRenderer<Client = kube::Client, Error = anyhow::Error>>,
}

impl InstanceService {
    pub(crate) fn new(
        renderer: Arc<dyn InstanceRenderer<Client = kube::Client, Error = anyhow::Error>>,
    ) -> Self {
        Self { renderer }
    }

    pub(crate) fn render(&self, instance: &InstanceSpec) -> Vec<Value> {
        self.renderer.render(instance, &[], &[])
    }

    pub(crate) async fn reconcile_plan(
        &self,
        instance: &InstanceSpec,
        client: kube::Client,
    ) -> anyhow::Result<service_k8s::service::ReconcilePlan> {
        // The metrics agent lists Kubernetes Pods in every auth mode.
        let (api_cidrs, api_ports) = self.renderer.discover_api_endpoint(client.clone()).await?;
        let children = self.renderer.render(instance, &api_cidrs, &api_ports);
        // Only a GCS destination or an alerting host list renders an FQDN policy.
        if children
            .iter()
            .any(|child| child["kind"] == "FQDNNetworkPolicy")
        {
            self.renderer.ensure_fqdn_support(client).await?;
        }
        Ok(service_k8s::service::ReconcilePlan::new(
            children,
            Value::Null,
        ))
    }

    pub(crate) fn readiness_targets(&self, instance: &InstanceSpec) -> Vec<ReadinessTarget> {
        let name = instance.name.clone();
        if instance.settings.topology == InstanceTopology::SingleNode {
            return vec![ReadinessTarget::new("StatefulSet", format!("{name}-all"))];
        }
        vec![
            ReadinessTarget::new("StatefulSet", format!("{name}-store")),
            ReadinessTarget::new("StatefulSet", format!("{name}-control")),
            ReadinessTarget::new("Deployment", format!("{name}-gateway")),
            ReadinessTarget::new("Deployment", format!("{name}-query")),
            ReadinessTarget::new("DaemonSet", format!("{name}-agent")),
        ]
    }

    pub(crate) fn status_patch(&self, instance: &InstanceSpec, ready: &ReadyFacts) -> Value {
        let name = instance.name.clone();
        let (
            phase,
            message,
            ready_replicas,
            desired_replicas_per_shard,
            current_shards,
            current_per_shard,
            restored_phase,
        ) = if instance.settings.topology == InstanceTopology::SingleNode {
            let all_ready = ready.get(&format!("{name}-all")).max(0);
            let available = all_ready >= 1;
            (
                if available { "Ready" } else { "Pending" },
                format!("all {all_ready}/1 ready"),
                all_ready,
                1,
                u32::from(available),
                all_ready.min(1) as u32,
                if available { "Restored" } else { "Requested" },
            )
        } else {
            let store_ready = ready.get(&format!("{name}-store")).max(0);
            let control_ready = ready.get(&format!("{name}-control")).max(0);
            let gateway_ready = ready.get(&format!("{name}-gateway")).max(0);
            let query_ready = ready.get(&format!("{name}-query")).max(0);
            let agent_ready = ready.get(&format!("{name}-agent")).max(0);
            let ready_replicas =
                store_ready + control_ready + gateway_ready + query_ready + agent_ready;
            let desired_replicas_per_shard = instance.settings.replicas_per_shard;
            let supported_topology =
                desired_replicas_per_shard == 3 && instance.settings.voter_count == 3;
            let (phase, message) = if !supported_topology {
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
            let restored_phase = if store_ready >= 3 {
                "Restored"
            } else if store_ready > 0 {
                "Restoring"
            } else {
                "Requested"
            };
            (
                phase,
                message,
                ready_replicas,
                desired_replicas_per_shard,
                u32::from(store_ready >= 2),
                store_ready.min(3) as u32,
                restored_phase,
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
            .map(|uri| (restored_phase, uri))
            .unwrap_or(("NotRequested", ""));
        json!({
            "status": {
                "phase": phase,
                "observedGeneration": instance.generation,
                "readyReplicas": ready_replicas,
                "desiredShardCount": 1,
                "currentShardCount": current_shards,
                "desiredReplicasPerShard": desired_replicas_per_shard,
                "currentReadyReplicasPerShard": current_per_shard,
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
        let desired = if instance.settings.topology == InstanceTopology::SingleNode {
            // A single-node instance renders no store, control or backup
            // policy, so those must go; only the all-role Pod's policy stays.
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
        desired
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

    pub(crate) fn cluster_scoped_children(
        &self,
        instance: &InstanceSpec,
    ) -> Vec<service_k8s::service::ClusterScopedChild> {
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
