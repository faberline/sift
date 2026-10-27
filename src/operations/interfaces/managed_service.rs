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
