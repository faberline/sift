//! The CRD adapter for the instance rendering and status use cases.

use crate::operations::application::instance_service::InstanceService;
use crate::operations::interfaces::crd::sift_spec::Sift;
use serde_json::Value;
use service_k8s::{ManagedService, ReadinessTarget, ReadyFacts};

impl ManagedService for Sift {
    const MANAGER: &'static str = "sift-operator";

    fn render(&self) -> Vec<Value> {
        InstanceService::default().render(&self.into())
    }

    fn reconcile_plan(
        &self,
        client: kube::Client,
    ) -> impl std::future::Future<Output = anyhow::Result<service_k8s::service::ReconcilePlan>> + Send
    {
        let instance = self.into();
        async move {
            InstanceService::default()
                .reconcile_plan(&instance, client)
                .await
        }
    }

    fn readiness_targets(&self) -> Vec<ReadinessTarget> {
        InstanceService::default().readiness_targets(&self.into())
    }
    fn status_patch(&self, ready: &ReadyFacts) -> Value {
        InstanceService::default().status_patch(&self.into(), ready)
    }
    fn prunes(&self) -> Vec<service_k8s::service::PruneTarget> {
        InstanceService::default().prunes(&self.into())
    }
    fn cluster_scoped_children(&self) -> Vec<service_k8s::service::ClusterScopedChild> {
        InstanceService::default().cluster_scoped_children(&self.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::operations::application::instance_service::{
        precreated_auth_delegator_from, InstanceService,
    };
    use crate::operations::interfaces::crd::sift_spec::Sift;
    use crate::shared_kernel::instance_spec::InstanceSpec;
    use serde_json::json;
    use service_k8s::{ConditionStatus, ManagedService};

    fn instance() -> InstanceSpec {
        let sift: Sift = serde_json::from_value(json!({
            "apiVersion": "sift.axiom.dev/v1alpha1",
            "kind": "Sift",
            "metadata": {
                "name": "events",
                "namespace": "observability",
                "uid": "sift-owner-uid",
                "generation": 7
            },
            "spec": {
                "image": "example.invalid/sift:test",
                "peerTlsSecret": "sift-peer-tls",
                "replicasPerShard": 3,
                "voterCount": 3,
                "dataSize": "1Gi",
                "auth": "kubernetes"
            }
        }))
        .expect("decode Sift resource");
        InstanceSpec::from(&sift)
    }

    #[test]
    fn precreated_mode_skips_the_auth_delegator_binding() {
        let spec = instance();
        let service = InstanceService::default().with_precreated_auth_delegator(true);

        assert!(service.cluster_scoped_children(&spec).is_empty());
        let objects = service.render(&spec);
        assert!(!objects
            .iter()
            .any(|object| object["kind"] == "ClusterRoleBinding"));
    }

    #[test]
    fn auth_delegator_binding_is_rendered_by_default() {
        let spec = instance();
        let service = InstanceService::default().with_precreated_auth_delegator(false);

        let children = service.cluster_scoped_children(&spec);
        assert_eq!(children.len(), 1);
        assert_eq!(
            children[0].name(),
            "sift.observability.events.auth-delegator"
        );
        assert!(children[0].desired());
        let objects = service.render(&spec);
        assert!(objects.iter().any(|object| {
            object["kind"] == "ClusterRoleBinding"
                && object["metadata"]["name"] == "sift.observability.events.auth-delegator"
        }));
    }

    #[test]
    fn precreated_flag_is_true_only_for_the_word_true() {
        assert!(precreated_auth_delegator_from(Some("true")));
        assert!(!precreated_auth_delegator_from(None));
        for value in ["", "false", "1", "TRUE"] {
            assert!(
                !precreated_auth_delegator_from(Some(value)),
                "{value:?} must not turn the mode on"
            );
        }
    }

    #[test]
    fn missing_precreated_binding_sets_condition() {
        let facts = InstanceService::default()
            .conditions(&instance(), &json!({"authDelegatorBindingFound": false}));

        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].type_, "AuthDelegatorBinding");
        assert_eq!(facts[0].status, ConditionStatus::False);
        assert_eq!(facts[0].reason, "PrecreatedBindingMissing");
        assert!(
            facts[0]
                .message
                .contains("sift.observability.events.auth-delegator"),
            "the message names the binding: {:?}",
            facts[0].message
        );
    }

    #[test]
    fn found_precreated_binding_sets_true_condition() {
        let facts = InstanceService::default()
            .conditions(&instance(), &json!({"authDelegatorBindingFound": true}));

        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].type_, "AuthDelegatorBinding");
        assert_eq!(facts[0].status, ConditionStatus::True);
        assert_eq!(facts[0].reason, "PrecreatedBindingFound");
    }

    #[test]
    fn no_condition_without_the_precreated_lookup() {
        let facts = InstanceService::default().conditions(&instance(), &serde_json::Value::Null);

        assert!(facts.is_empty(), "default mode writes no condition");
    }

    #[test]
    fn sift_status_keeps_conditions_for_the_projection() {
        let sift: Sift = serde_json::from_value(json!({
            "apiVersion": "sift.axiom.dev/v1alpha1",
            "kind": "Sift",
            "metadata": {
                "name": "events",
                "namespace": "observability",
                "uid": "sift-owner-uid",
                "generation": 7
            },
            "spec": {
                "image": "example.invalid/sift:test",
                "peerTlsSecret": "sift-peer-tls",
                "replicasPerShard": 3,
                "voterCount": 3,
                "dataSize": "1Gi",
                "auth": "kubernetes"
            },
            "status": {
                "conditions": [{
                    "type": "AuthDelegatorBinding",
                    "status": "False",
                    "reason": "PrecreatedBindingMissing",
                    "message": "m",
                    "lastTransitionTime": "2026-01-01T00:00:00Z"
                }]
            }
        }))
        .expect("decode Sift resource");

        let observed = ManagedService::observed_conditions(&sift);

        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].type_, "AuthDelegatorBinding");
        assert_eq!(observed[0].status, "False");
        assert_eq!(observed[0].reason, "PrecreatedBindingMissing");
        assert_eq!(observed[0].message, "m");
        assert_eq!(observed[0].last_transition_time, "2026-01-01T00:00:00Z");
    }
}
