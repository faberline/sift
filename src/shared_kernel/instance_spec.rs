//! A resolved Sift instance, independent of the Kubernetes CRD and client.

use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub(crate) struct InstanceSpec {
    pub name: String,
    pub namespace: String,
    pub uid: Option<String>,
    pub generation: i64,
    pub settings: InstanceSettings,
}

#[derive(Clone, Debug)]
pub(crate) struct InstanceSettings {
    pub image: String,
    pub peer_tls_secret: String,
    pub replicas_per_shard: u32,
    pub voter_count: u32,
    pub data_size: Option<String>,
    pub archive: Option<InstanceArchive>,
    pub bootstrap: InstanceBootstrap,
    pub storage: InstanceStorage,
    pub ingest: InstanceIngest,
    pub placement: InstancePlacement,
    pub auth: InstanceAuth,
    pub topology: InstanceTopology,
    pub logs: InstanceLogs,
    pub retention_days: u32,
    pub metrics: InstanceMetrics,
    pub tokens_secret: Option<String>,
    pub backup: Option<InstanceBackup>,
    pub alerting: InstanceAlerting,
    pub gcp_project_id: String,
    pub gke_cluster_name: String,
    pub gke_location: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum InstanceAuth {
    Off,
    Required,
    Kubernetes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstanceTopology {
    Ha,
    SingleNode,
}

#[derive(Clone, Debug)]
pub(crate) struct InstanceLogs {
    pub namespaces: Vec<String>,
    pub pod_label: Option<String>,
    /// `false` renders no log agent and no metrics agent.
    pub agent_enabled: bool,
}

impl Default for InstanceLogs {
    fn default() -> Self {
        Self {
            namespaces: Vec::new(),
            pod_label: None,
            agent_enabled: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct InstanceMetrics {
    pub namespaces: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct InstanceBackup {
    pub schedule: String,
    pub destination: String,
    pub retention_secs: Option<u64>,
    pub admin_token_secret: Option<String>,
}
#[derive(Clone, Debug)]
pub(crate) struct InstanceAlerting {
    pub webhook_secret: Option<String>,
    pub allowed_hosts: Vec<String>,
}
#[derive(Clone, Debug)]
pub(crate) struct InstanceArchive {
    pub destination: String,
}
#[derive(Clone, Debug)]
pub(crate) struct InstanceBootstrap {
    pub archive_manifest_uri: Option<String>,
}
#[derive(Clone, Debug)]
pub(crate) struct InstanceStorage {
    pub store_size: String,
    pub control_size: String,
    pub gateway_size: String,
    pub query_size: String,
    pub storage_class: String,
}
#[derive(Clone, Debug)]
pub(crate) struct InstanceIngest {
    pub max_items_per_minute: u64,
    pub max_concurrent_requests: u32,
}
#[derive(Clone, Debug)]
pub(crate) struct InstancePlacement {
    pub node_selector: BTreeMap<String, String>,
    pub tolerations: Vec<InstanceToleration>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct InstanceToleration {
    pub key: Option<String>,
    pub operator: Option<String>,
    pub value: Option<String>,
    pub effect: Option<String>,
    pub toleration_seconds: Option<i64>,
}
