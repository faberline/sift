//! Render workloads and report instance state through the platform port.

use crate::operations::domain::instance_names::{
    APP, AUTH_DELEGATION_COMPONENT, BACKUP_COMPONENT, STORE_COMPONENT,
};
use crate::operations::domain::instance_renderer::InstanceRenderer;
use crate::shared_kernel::instance_spec::{InstanceAuth as AuthMode, InstanceSpec};
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
        let (api_cidrs, api_ports) = if matches!(instance.settings.auth, AuthMode::Kubernetes) {
            self.renderer.discover_api_endpoint(client).await?
        } else {
            (Vec::new(), Vec::new())
        };
        Ok(service_k8s::service::ReconcilePlan {
            children: self.renderer.render(instance, &api_cidrs, &api_ports),
            context: Value::Null,
        })
    }

    pub(crate) fn readiness_targets(&self, instance: &InstanceSpec) -> Vec<ReadinessTarget> {
        let name = instance.name.clone();
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

    pub(crate) fn status_patch(&self, instance: &InstanceSpec, ready: &ReadyFacts) -> Value {
        let name = instance.name.clone();
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
                "observedGeneration": instance.generation,
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

    pub(crate) fn cluster_scoped_children(
        &self,
        instance: &InstanceSpec,
    ) -> Vec<service_k8s::service::ClusterScopedChild> {
        let Some(uid) = instance.uid.as_deref() else {
            return Vec::new();
        };
        let name = instance.name.clone();
        let namespace = instance.namespace.clone();
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
            desired: matches!(instance.settings.auth, AuthMode::Kubernetes),
        }]
    }
}
