//! Convert the already-defaulted CRD into plain instance settings.

use crate::operations::interfaces::crd::sift_spec::{AuthMode, Sift};
use crate::shared_kernel::instance_spec::{
    InstanceArchive, InstanceAuth, InstanceBackup, InstanceBootstrap, InstanceIngest,
    InstancePlacement, InstanceSettings, InstanceSpec, InstanceStorage,
};
use kube::ResourceExt;

impl From<&Sift> for InstanceSpec {
    fn from(sift: &Sift) -> Self {
        let spec = &sift.spec;
        Self {
            name: sift.name_any(),
            namespace: sift.namespace().unwrap_or_else(|| "default".to_string()),
            uid: sift.metadata.uid.clone(),
            generation: sift.metadata.generation.unwrap_or(0),
            settings: InstanceSettings {
                image: spec.image.clone(),
                peer_tls_secret: spec.peer_tls_secret.clone(),
                replicas_per_shard: spec.replicas_per_shard,
                voter_count: spec.voter_count,
                data_size: spec.data_size.clone(),
                archive: spec.archive.as_ref().map(|v| InstanceArchive {
                    destination: v.destination.clone(),
                }),
                bootstrap: InstanceBootstrap {
                    archive_manifest_uri: spec.bootstrap.archive_manifest_uri.clone(),
                },
                storage: InstanceStorage {
                    store_size: spec.storage.store_size.clone(),
                    control_size: spec.storage.control_size.clone(),
                    gateway_size: spec.storage.gateway_size.clone(),
                    query_size: spec.storage.query_size.clone(),
                },
                ingest: InstanceIngest {
                    max_items_per_minute: spec.ingest.max_items_per_minute,
                    max_concurrent_requests: spec.ingest.max_concurrent_requests,
                },
                placement: InstancePlacement {
                    node_selector: spec.placement.node_selector.clone(),
                },
                auth: match spec.auth {
                    AuthMode::Off => InstanceAuth::Off,
                    AuthMode::Required => InstanceAuth::Required,
                    AuthMode::Kubernetes => InstanceAuth::Kubernetes,
                },
                tokens_secret: spec.tokens_secret.clone(),
                backup: spec.backup.as_ref().map(|v| InstanceBackup {
                    schedule: v.schedule.clone(),
                    destination: v.destination.clone(),
                    retention_secs: v.retention_secs,
                    admin_token_secret: v.admin_token_secret.clone(),
                }),
                gcp_project_id: spec.gcp_project_id.clone(),
                gke_cluster_name: spec.gke_cluster_name.clone(),
                gke_location: spec.gke_location.clone(),
            },
        }
    }
}
