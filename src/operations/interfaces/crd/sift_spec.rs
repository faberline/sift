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
    /// Secret with tls.crt, tls.key, and ca.crt for the dedicated Raft port.
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
    pub auth: AuthMode,
    #[serde(default)]
    pub tokens_secret: Option<String>,
    #[serde(default)]
    pub backup: Option<BackupSpec>,
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
}

impl Default for StorageSpec {
    fn default() -> Self {
        Self {
            store_size: "50Gi".to_string(),
            control_size: "5Gi".to_string(),
            gateway_size: "2Gi".to_string(),
            query_size: "2Gi".to_string(),
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
