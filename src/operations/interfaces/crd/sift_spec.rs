//! The Sift custom resource: its spec with the backup, archive, bootstrap,
//! storage, ingest, placement and auth sections, and the status the operator
//! reports.

use std::collections::BTreeMap;

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "sift.axiom.dev",
    version = "v1alpha1",
    kind = "Sift",
    plural = "sifts",
    namespaced,
    status = "SiftStatus"
)]
#[serde(rename_all = "camelCase")]
pub struct SiftSpec {
    pub image: String,
    /// Image of the init containers that prepare the data root. Empty means the pinned default busybox image.
    #[serde(default)]
    pub init_image: Option<String>,
    /// Secret with tls.crt, tls.key, and ca.crt for the dedicated Raft port.
    /// Required for `topology: ha`; `singleNode` does not use it.
    #[serde(default)]
    pub peer_tls_secret: String,
    #[serde(default = "three")]
    pub replicas_per_shard: u32,
    #[serde(default = "three")]
    pub voter_count: u32,
    /// Deprecated single-size compatibility field. New resources use
    /// `storage`, which gives each stateful role its own bounded PVC.
    #[serde(default)]
    pub data_size: Option<String>,
    #[serde(default)]
    pub archive: Option<ArchiveSpec>,
    #[serde(default)]
    pub bootstrap: BootstrapSpec,
    #[serde(default)]
    pub storage: StorageSpec,
    #[serde(default)]
    pub ingest: IngestSpec,
    #[serde(default)]
    pub placement: PlacementSpec,
    #[serde(default)]
    pub resources: ResourcesSpec,
    #[serde(default)]
    pub auth: AuthMode,
    #[serde(default)]
    pub topology: Topology,
    #[serde(default)]
    pub logs: LogsSpec,
    /// Days of events Sift keeps and accepts, 1 to 180. The CRD schema enforces the range.
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    #[serde(default)]
    pub metrics: MetricsSpec,
    #[serde(default)]
    pub tokens_secret: Option<String>,
    #[serde(default)]
    pub backup: Option<BackupSpec>,
    #[serde(default)]
    pub alerting: AlertingSpec,
    #[serde(default)]
    pub gcp_project_id: String,
    #[serde(default)]
    pub gke_cluster_name: String,
    #[serde(default)]
    pub gke_location: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    #[default]
    Off,
    Required,
    Kubernetes,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Topology {
    #[default]
    Ha,
    SingleNode,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BackupSpec {
    pub schedule: String,
    pub destination: String,
    #[serde(default)]
    pub retention_secs: Option<u64>,
    #[serde(default)]
    pub admin_token_secret: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LogsAgent {
    #[default]
    Enabled,
    Disabled,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct LogsSpec {
    /// Kubernetes namespaces whose Pod logs the log agent collects.
    /// Empty means the namespace of the Sift instance only.
    pub namespaces: Vec<String>,
    /// Collect logs only from Pods that carry this label (`key=value`). Empty means every Pod of the listed namespaces.
    pub pod_label: Option<String>,
    /// `enabled` runs the log and metrics agents. `disabled` runs no agent, for
    /// logs, metrics and traces that arrive by OTLP.
    pub agent: LogsAgent,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct MetricsSpec {
    /// Kubernetes namespaces whose Pods the metrics agent scrapes.
    /// Empty means the namespace of the Sift instance only.
    pub namespaces: Vec<String>,
}

/// Compute requests and limits of the Sift pods. The keys are `cpu` and `memory`.
/// Empty `requests` means the default `cpu: 100m`, `memory: 256Mi`.
/// Empty `limits` means no limits.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct ResourcesSpec {
    /// Requests of every role. Keys are `cpu` and `memory`.
    pub requests: BTreeMap<String, String>,
    /// Limits of every role. Keys are `cpu` and `memory`.
    pub limits: BTreeMap<String, String>,
    /// Values for the `all` role group. Overrides the common values.
    pub all: Option<RoleResourcesSpec>,
    /// Values for the `store` role. Overrides the common values.
    pub store: Option<RoleResourcesSpec>,
    /// Values for the `control` role. Overrides the common values.
    pub control: Option<RoleResourcesSpec>,
    /// Values for the `gateway` role. Overrides the common values.
    pub gateway: Option<RoleResourcesSpec>,
    /// Values for the `query` role. Overrides the common values.
    pub query: Option<RoleResourcesSpec>,
}

/// Requests or limits of one role. A set field, even an empty one, replaces the common value.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct RoleResourcesSpec {
    /// Requests of this role. Replaces the common requests when set.
    pub requests: Option<BTreeMap<String, String>>,
    /// Limits of this role. Replaces the common limits when set.
    pub limits: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct AlertingSpec {
    /// Secret keys are reference-name.json with an HTTPS URL and optional bearer_token.
    pub webhook_secret: Option<String>,
    pub allowed_hosts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSpec {
    pub destination: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct BootstrapSpec {
    pub archive_manifest_uri: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct StorageSpec {
    pub store_size: String,
    pub control_size: String,
    pub gateway_size: String,
    pub query_size: String,
    pub storage_class: String,
}

impl Default for StorageSpec {
    fn default() -> Self {
        Self {
            store_size: "50Gi".to_string(),
            control_size: "5Gi".to_string(),
            gateway_size: "2Gi".to_string(),
            query_size: "2Gi".to_string(),
            storage_class: String::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct IngestSpec {
    pub max_items_per_minute: u64,
    pub max_concurrent_requests: u32,
}

impl Default for IngestSpec {
    fn default() -> Self {
        Self {
            max_items_per_minute: 720_000,
            max_concurrent_requests: 32,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct PlacementSpec {
    pub node_selector: BTreeMap<String, String>,
    pub tolerations: Vec<TolerationSpec>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct TolerationSpec {
    pub key: Option<String>,
    pub operator: Option<String>,
    pub value: Option<String>,
    pub effect: Option<String>,
    pub toleration_seconds: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct SiftStatus {
    pub phase: String,
    pub observed_generation: i64,
    pub ready_replicas: i64,
    pub desired_shard_count: u32,
    pub current_shard_count: u32,
    pub desired_replicas_per_shard: u32,
    pub current_ready_replicas_per_shard: u32,
    pub backup_phase: String,
    pub backup_message: String,
    pub archive_phase: String,
    pub archive_watermark: u64,
    pub last_archive_manifest: String,
    pub restore_phase: String,
    pub restore_source_manifest: String,
    pub backpressure: String,
    pub last_data_error: String,
    pub message: String,
}

fn three() -> u32 {
    3
}

fn default_retention_days() -> u32 {
    180
}

#[cfg(test)]
mod tests {
    use super::{SiftSpec, Topology};

    fn spec_with(extra: &str) -> Result<SiftSpec, serde_json::Error> {
        let mut json =
            String::from(r#"{"image":"example.invalid/sift:test","peerTlsSecret":"sift-peer-tls""#);
        if !extra.is_empty() {
            json.push(',');
            json.push_str(extra);
        }
        json.push('}');
        serde_json::from_str(&json)
    }

    fn spec_without_peer(extra: &str) -> Result<SiftSpec, serde_json::Error> {
        let mut json = String::from(r#"{"image":"example.invalid/sift:test""#);
        if !extra.is_empty() {
            json.push(',');
            json.push_str(extra);
        }
        json.push('}');
        serde_json::from_str(&json)
    }

    #[test]
    fn peer_tls_secret_is_optional_for_single_node() {
        let spec = spec_without_peer(r#""topology":"singleNode""#).unwrap();
        assert_eq!(spec.topology, Topology::SingleNode);
        assert_eq!(spec.peer_tls_secret, "");
    }

    #[test]
    fn gcp_fields_default_to_empty() {
        let spec = spec_with("").unwrap();
        assert_eq!(spec.gcp_project_id, "");
        assert_eq!(spec.gke_cluster_name, "");
        assert_eq!(spec.gke_location, "");
    }

    #[test]
    fn topology_defaults_to_ha() {
        assert_eq!(spec_with("").unwrap().topology, Topology::Ha);
    }

    #[test]
    fn topology_single_node_is_accepted() {
        let spec = spec_with(r#""topology":"singleNode""#).unwrap();
        assert_eq!(spec.topology, Topology::SingleNode);
    }

    #[test]
    fn topology_unknown_value_is_refused() {
        assert!(spec_with(r#""topology":"cluster""#).is_err());
    }

    #[test]
    fn logs_default_is_empty() {
        assert!(spec_with("").unwrap().logs.namespaces.is_empty());
    }

    #[test]
    fn logs_namespaces_are_read() {
        let spec = spec_with(r#""logs":{"namespaces":["team-a","team-b"]}"#).unwrap();
        assert_eq!(spec.logs.namespaces, ["team-a", "team-b"]);
    }

    #[test]
    fn logs_pod_label_default_is_none() {
        assert!(spec_with("").unwrap().logs.pod_label.is_none());
    }

    #[test]
    fn logs_pod_label_is_read() {
        let spec = spec_with(r#""logs":{"podLabel":"sift.axiom.dev/logs=true"}"#).unwrap();
        assert_eq!(
            spec.logs.pod_label,
            Some("sift.axiom.dev/logs=true".to_string())
        );
    }

    #[test]
    fn logs_agent_default_is_enabled() {
        assert_eq!(spec_with("").unwrap().logs.agent, super::LogsAgent::Enabled);
    }

    #[test]
    fn metrics_default_is_empty() {
        assert!(spec_with("").unwrap().metrics.namespaces.is_empty());
    }

    #[test]
    fn metrics_namespaces_are_read() {
        let spec = spec_with(r#""metrics":{"namespaces":["team-a","team-b"]}"#).unwrap();
        assert_eq!(spec.metrics.namespaces, ["team-a", "team-b"]);
    }

    #[test]
    fn resources_default_is_empty() {
        let spec = spec_with("").unwrap();
        assert!(spec.resources.requests.is_empty());
        assert!(spec.resources.limits.is_empty());
        assert!(spec.resources.store.is_none());
    }

    #[test]
    fn resources_are_read() {
        let spec = spec_with(
            r#""resources":{"requests":{"cpu":"2","memory":"10Gi"},"limits":{"memory":"12Gi"},"store":{"limits":{"memory":"16Gi"}}}"#,
        )
        .unwrap();
        assert_eq!(spec.resources.requests["memory"], "10Gi");
        assert_eq!(spec.resources.limits["memory"], "12Gi");
        let store = spec.resources.store.unwrap();
        assert_eq!(store.limits.as_ref().unwrap()["memory"], "16Gi");
        assert!(store.requests.is_none());
    }

    #[test]
    fn retention_days_default_is_180() {
        assert_eq!(spec_with("").unwrap().retention_days, 180);
    }

    #[test]
    fn placement_tolerations_are_read() {
        let spec = spec_with(
            r#""placement":{"tolerations":[{"key":"faberline.dev/pool","operator":"Equal","value":"sift-server","effect":"NoSchedule"}]}"#,
        )
        .unwrap();
        assert_eq!(spec.placement.tolerations.len(), 1);
        let toleration = &spec.placement.tolerations[0];
        assert_eq!(toleration.key.as_deref(), Some("faberline.dev/pool"));
        assert_eq!(toleration.effect.as_deref(), Some("NoSchedule"));
        assert_eq!(toleration.toleration_seconds, None);
    }

    #[test]
    fn placement_tolerations_default_is_empty() {
        assert!(spec_with("").unwrap().placement.tolerations.is_empty());
    }

    #[test]
    fn placement_toleration_unknown_field_is_refused() {
        assert!(spec_with(r#""placement":{"tolerations":[{"key":"a","foo":"b"}]}"#).is_err());
    }

    #[test]
    fn storage_class_is_read_and_defaults_to_empty() {
        let spec = spec_with(r#""storage":{"storageClass":"dynamic-rwo"}"#).unwrap();
        assert_eq!(spec.storage.storage_class, "dynamic-rwo");
        assert_eq!(spec_with("").unwrap().storage.storage_class, "");
    }

    #[test]
    fn retention_days_is_read() {
        let spec = spec_with(r#""retentionDays":30"#).unwrap();
        assert_eq!(spec.retention_days, 30);
    }
}
