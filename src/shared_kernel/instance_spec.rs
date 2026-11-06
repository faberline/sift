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
    pub tokens_secret: Option<String>,
    pub backup: Option<InstanceBackup>,
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

#[derive(Clone, Debug)]
pub(crate) struct InstanceBackup {
    pub schedule: String,
    pub destination: String,
    pub retention_secs: Option<u64>,
    pub admin_token_secret: Option<String>,
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
}
#[derive(Clone, Debug)]
pub(crate) struct InstanceIngest {
    pub max_items_per_minute: u64,
    pub max_concurrent_requests: u32,
}
#[derive(Clone, Debug)]
pub(crate) struct InstancePlacement {
    pub node_selector: BTreeMap<String, String>,
}
