//! Convert the already-defaulted CRD into plain instance settings.

use std::collections::BTreeMap;

use crate::operations::interfaces::crd::sift_spec::{
    AuthMode, LogsAgent, Sift, SiftSpec, Topology,
};
use crate::shared_kernel::instance_spec::{
    InstanceAlerting, InstanceArchive, InstanceAuth, InstanceBackup, InstanceBootstrap,
    InstanceIngest, InstanceLogs, InstanceMetrics, InstancePlacement, InstanceResources,
    InstanceRoleResources, InstanceSettings, InstanceSpec, InstanceStorage, InstanceToleration,
    InstanceTopology,
};
use kube::ResourceExt;

impl From<&Sift> for InstanceSpec {
    fn from(sift: &Sift) -> Self {
        let spec = &sift.spec;
        let mut roles = BTreeMap::new();
        for (name, role) in [
            ("all", &spec.resources.all),
            ("store", &spec.resources.store),
            ("control", &spec.resources.control),
            ("gateway", &spec.resources.gateway),
            ("query", &spec.resources.query),
        ] {
            if let Some(role) = role {
                roles.insert(
                    name.to_string(),
                    InstanceRoleResources {
                        requests: role.requests.clone(),
                        limits: role.limits.clone(),
                    },
                );
            }
        }
        Self {
            name: sift.name_any(),
            namespace: sift.namespace().unwrap_or_else(|| "default".to_string()),
            uid: sift.metadata.uid.clone(),
            generation: sift.metadata.generation.unwrap_or(0),
            settings: InstanceSettings {
                image: spec.image.clone(),
                init_image: spec.init_image.clone(),
                peer_tls_secret: spec.peer_tls_secret.clone(),
                peer_tls_secret_provider_class: spec.peer_tls_secret_provider_class.clone(),
                secrets_csi_driver: spec.secrets_csi_driver.clone(),
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
                    storage_class: spec.storage.storage_class.clone(),
                },
                ingest: InstanceIngest {
                    max_items_per_minute: spec.ingest.max_items_per_minute,
                    max_concurrent_requests: spec.ingest.max_concurrent_requests,
                },
                placement: InstancePlacement {
                    node_selector: spec.placement.node_selector.clone(),
                    tolerations: spec
                        .placement
                        .tolerations
                        .iter()
                        .map(|toleration| InstanceToleration {
                            key: toleration.key.clone(),
                            operator: toleration.operator.clone(),
                            value: toleration.value.clone(),
                            effect: toleration.effect.clone(),
                            toleration_seconds: toleration.toleration_seconds,
                        })
                        .collect(),
                },
                resources: InstanceResources {
                    requests: spec.resources.requests.clone(),
                    limits: spec.resources.limits.clone(),
                    roles,
                },
                auth: match spec.auth {
                    AuthMode::Off => InstanceAuth::Off,
                    AuthMode::Required => InstanceAuth::Required,
                    AuthMode::Kubernetes => InstanceAuth::Kubernetes,
                },
                topology: match spec.topology {
                    Topology::Ha => InstanceTopology::Ha,
                    Topology::SingleNode => InstanceTopology::SingleNode,
                },
                logs: InstanceLogs {
                    namespaces: spec.logs.namespaces.clone(),
                    pod_label: spec.logs.pod_label.clone(),
                    agent_enabled: !matches!(spec.logs.agent, LogsAgent::Disabled),
                },
                retention_days: spec.retention_days,
                metrics: InstanceMetrics {
                    namespaces: spec.metrics.namespaces.clone(),
                },
                tokens_secret: spec.tokens_secret.clone(),
                tokens_secret_provider_class: spec.tokens_secret_provider_class.clone(),
                image_pull_secrets: spec.image_pull_secrets.clone(),
                alerting: InstanceAlerting {
                    webhook_secret: spec.alerting.webhook_secret.clone(),
                    webhook_secret_provider_class: spec
                        .alerting
                        .webhook_secret_provider_class
                        .clone(),
                    allowed_hosts: spec.alerting.allowed_hosts.clone(),
                },
                backup: spec.backup.as_ref().map(|v| InstanceBackup {
                    schedule: v.schedule.clone(),
                    destination: v.destination.clone(),
                    retention_secs: v.retention_secs,
                    admin_token_secret: v.admin_token_secret.clone(),
                    admin_token_secret_provider_class: v.admin_token_secret_provider_class.clone(),
                }),
                gcp_project_id: spec.gcp_project_id.clone(),
                gke_cluster_name: spec.gke_cluster_name.clone(),
                gke_location: spec.gke_location.clone(),
            },
        }
    }
}

/// Checks the pairs of Secret fields. The CRD rules check the same pairs at admission.
#[allow(dead_code)] // the CEL rule of the CRD is the admission check; no caller yet
pub(crate) fn validate_secret_sources(spec: &SiftSpec) -> Result<(), String> {
    let peer_secret = !spec.peer_tls_secret.is_empty();
    let peer_class = spec.peer_tls_secret_provider_class.is_some();
    if peer_secret && peer_class {
        return Err(
            "spec.peerTlsSecret and spec.peerTlsSecretProviderClass are both set; set at most one"
                .into(),
        );
    }
    if !peer_secret && !peer_class && !matches!(spec.topology, Topology::SingleNode) {
        return Err("neither spec.peerTlsSecret nor spec.peerTlsSecretProviderClass is set; set exactly one when spec.topology is ha".into());
    }
    if spec.tokens_secret.is_some() && spec.tokens_secret_provider_class.is_some() {
        return Err(
            "spec.tokensSecret and spec.tokensSecretProviderClass are both set; set at most one"
                .into(),
        );
    }
    if spec.alerting.webhook_secret.is_some()
        && spec.alerting.webhook_secret_provider_class.is_some()
    {
        return Err("spec.alerting.webhookSecret and spec.alerting.webhookSecretProviderClass are both set; set at most one".into());
    }
    if let Some(backup) = &spec.backup {
        if backup.admin_token_secret.is_some() && backup.admin_token_secret_provider_class.is_some()
        {
            return Err("spec.backup.adminTokenSecret and spec.backup.adminTokenSecretProviderClass are both set; set at most one".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{InstanceSpec, Sift};
    use crate::operations::interfaces::crd::sift_spec::SiftSpec;
    use crate::shared_kernel::instance_spec::InstanceTopology;

    fn sift(topology: Option<&str>) -> Sift {
        let mut value = serde_json::json!({
            "image": "example.invalid/sift:test",
            "peerTlsSecret": "sift-peer-tls"
        });
        if let Some(topology) = topology {
            value["topology"] = serde_json::Value::String(topology.into());
        }
        let spec: SiftSpec = serde_json::from_value(value).unwrap();
        Sift::new("events", spec)
    }

    #[test]
    fn topology_reaches_the_instance_settings() {
        let settings = InstanceSpec::from(&sift(Some("singleNode"))).settings;
        assert_eq!(settings.topology, InstanceTopology::SingleNode);
    }

    #[test]
    fn topology_default_reaches_the_instance_settings_as_ha() {
        let settings = InstanceSpec::from(&sift(None)).settings;
        assert_eq!(settings.topology, InstanceTopology::Ha);
    }

    #[test]
    fn logs_agent_defaults_to_enabled_and_disabled_converts() {
        let plain = InstanceSpec::from(&sift(None)).settings;
        assert!(plain.logs.agent_enabled);

        let spec: SiftSpec = serde_json::from_value(serde_json::json!({
            "image": "example.invalid/sift:test",
            "peerTlsSecret": "sift-peer-tls",
            "logs": {"agent": "disabled"}
        }))
        .unwrap();
        let settings = InstanceSpec::from(&Sift::new("events", spec)).settings;
        assert!(!settings.logs.agent_enabled);
    }

    #[test]
    fn logs_namespaces_reach_the_instance_settings() {
        let spec: SiftSpec = serde_json::from_value(serde_json::json!({
            "image": "example.invalid/sift:test",
            "peerTlsSecret": "sift-peer-tls",
            "logs": {"namespaces": ["team-a"]}
        }))
        .unwrap();
        let settings = InstanceSpec::from(&Sift::new("events", spec)).settings;
        assert_eq!(settings.logs.namespaces, vec!["team-a".to_string()]);
    }

    #[test]
    fn logs_pod_label_reaches_the_instance_settings() {
        let spec: SiftSpec = serde_json::from_value(serde_json::json!({
            "image": "example.invalid/sift:test",
            "peerTlsSecret": "sift-peer-tls",
            "logs": {"podLabel": "sift.axiom.dev/logs=true"}
        }))
        .unwrap();
        let settings = InstanceSpec::from(&Sift::new("events", spec)).settings;
        assert_eq!(
            settings.logs.pod_label,
            Some("sift.axiom.dev/logs=true".to_string())
        );

        let plain: SiftSpec = serde_json::from_value(serde_json::json!({
            "image": "example.invalid/sift:test",
            "peerTlsSecret": "sift-peer-tls"
        }))
        .unwrap();
        let plain = InstanceSpec::from(&Sift::new("events", plain)).settings;
        assert_eq!(plain.logs.pod_label, None);
    }

    #[test]
    fn retention_days_reaches_the_instance_settings() {
        let spec: SiftSpec = serde_json::from_value(serde_json::json!({
            "image": "example.invalid/sift:test",
            "peerTlsSecret": "sift-peer-tls",
            "retentionDays": 30
        }))
        .unwrap();
        let settings = InstanceSpec::from(&Sift::new("events", spec)).settings;
        assert_eq!(settings.retention_days, 30);
    }

    #[test]
    fn metrics_namespaces_reach_the_instance_settings() {
        let spec: SiftSpec = serde_json::from_value(serde_json::json!({
            "image": "example.invalid/sift:test",
            "peerTlsSecret": "sift-peer-tls",
            "metrics": {"namespaces": ["team-a"]}
        }))
        .unwrap();
        let settings = InstanceSpec::from(&Sift::new("events", spec)).settings;
        assert_eq!(settings.metrics.namespaces, vec!["team-a".to_string()]);
    }

    #[test]
    fn toleration_and_storage_class_reach_the_instance_settings() {
        let spec: SiftSpec = serde_json::from_value(serde_json::json!({
            "image": "example.invalid/sift:test",
            "peerTlsSecret": "sift-peer-tls",
            "placement": {"tolerations": [{"key": "k", "operator": "Exists"}]},
            "storage": {"storageClass": "dynamic-rwo"}
        }))
        .unwrap();
        let settings = InstanceSpec::from(&Sift::new("events", spec)).settings;
        assert_eq!(settings.placement.tolerations.len(), 1);
        assert_eq!(settings.placement.tolerations[0].key, Some("k".to_string()));
        assert_eq!(settings.storage.storage_class, "dynamic-rwo");
    }

    #[test]
    fn image_pull_secrets_reach_the_instance_settings() {
        let spec: SiftSpec = serde_json::from_value(serde_json::json!({
            "image": "example.invalid/sift:test",
            "peerTlsSecret": "sift-peer-tls",
            "imagePullSecrets": ["ghcr-pull"]
        }))
        .unwrap();
        let settings = InstanceSpec::from(&Sift::new("events", spec)).settings;
        assert_eq!(settings.image_pull_secrets, ["ghcr-pull"]);
    }

    fn spec_of(value: serde_json::Value) -> SiftSpec {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn provider_class_only_peer_spec_is_accepted() {
        let spec = spec_of(serde_json::json!({
            "image": "example.invalid/sift:test",
            "peerTlsSecretProviderClass": "sift-peer-spc"
        }));
        assert_eq!(super::validate_secret_sources(&spec), Ok(()));
        let settings = InstanceSpec::from(&Sift::new("events", spec)).settings;
        assert_eq!(
            settings.peer_tls_secret_provider_class.as_deref(),
            Some("sift-peer-spc")
        );
        assert_eq!(settings.secrets_csi_driver, "secrets-store.csi.k8s.io");
    }

    #[test]
    fn existing_peer_secret_spec_still_converts() {
        let spec = spec_of(serde_json::json!({
            "image": "example.invalid/sift:test",
            "peerTlsSecret": "sift-peer-tls"
        }));
        assert_eq!(super::validate_secret_sources(&spec), Ok(()));
        let settings = InstanceSpec::from(&Sift::new("events", spec)).settings;
        assert_eq!(settings.peer_tls_secret, "sift-peer-tls");
    }

    #[test]
    fn peer_secret_and_provider_class_together_are_refused() {
        for topology in [None, Some("singleNode")] {
            let mut value = serde_json::json!({
                "image": "example.invalid/sift:test",
                "peerTlsSecret": "sift-peer-tls",
                "peerTlsSecretProviderClass": "sift-peer-spc"
            });
            if let Some(topology) = topology {
                value["topology"] = serde_json::json!(topology);
            }
            let spec = spec_of(value);
            let err = super::validate_secret_sources(&spec).unwrap_err();
            assert!(
                err.contains("peerTlsSecret") && err.contains("peerTlsSecretProviderClass"),
                "{err}"
            );
        }
    }

    #[test]
    fn neither_peer_field_is_refused_for_ha() {
        let spec = spec_of(serde_json::json!({"image": "example.invalid/sift:test"}));
        let err = super::validate_secret_sources(&spec).unwrap_err();
        assert!(
            err.contains("peerTlsSecret") && err.contains("peerTlsSecretProviderClass"),
            "{err}"
        );
    }

    #[test]
    fn single_node_without_peer_field_is_accepted() {
        let bare = spec_of(serde_json::json!({
            "image": "example.invalid/sift:test",
            "topology": "singleNode"
        }));
        assert_eq!(super::validate_secret_sources(&bare), Ok(()));
        let class_only = spec_of(serde_json::json!({
            "image": "example.invalid/sift:test",
            "topology": "singleNode",
            "peerTlsSecretProviderClass": "p"
        }));
        assert_eq!(super::validate_secret_sources(&class_only), Ok(()));
    }

    #[test]
    fn tokens_alerting_and_backup_pairs_are_refused_when_both_set() {
        let cases = [
            (
                serde_json::json!({
                    "image": "example.invalid/sift:test",
                    "peerTlsSecret": "t",
                    "tokensSecret": "a",
                    "tokensSecretProviderClass": "b"
                }),
                "tokensSecret",
                "tokensSecretProviderClass",
            ),
            (
                serde_json::json!({
                    "image": "example.invalid/sift:test",
                    "peerTlsSecret": "t",
                    "alerting": {"webhookSecret": "a", "webhookSecretProviderClass": "b"}
                }),
                "webhookSecret",
                "webhookSecretProviderClass",
            ),
            (
                serde_json::json!({
                    "image": "example.invalid/sift:test",
                    "peerTlsSecret": "t",
                    "backup": {
                        "schedule": "0 * * * *",
                        "destination": "gs://bucket/path",
                        "adminTokenSecret": "a",
                        "adminTokenSecretProviderClass": "b"
                    }
                }),
                "adminTokenSecret",
                "adminTokenSecretProviderClass",
            ),
        ];
        for (value, first, second) in cases {
            let spec = spec_of(value);
            let err = super::validate_secret_sources(&spec).unwrap_err();
            assert!(err.contains(first) && err.contains(second), "{err}");
        }
    }

    #[test]
    fn secrets_csi_driver_can_be_set() {
        let spec = spec_of(serde_json::json!({
            "image": "example.invalid/sift:test",
            "peerTlsSecret": "t",
            "secretsCsiDriver": "secrets-store-gke.csi.k8s.io"
        }));
        let settings = InstanceSpec::from(&Sift::new("events", spec)).settings;
        assert_eq!(settings.secrets_csi_driver, "secrets-store-gke.csi.k8s.io");
    }
}
