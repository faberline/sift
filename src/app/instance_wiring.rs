//! Select the Kubernetes implementation of the instance rendering port.

use crate::operations::application::instance_service::InstanceService;
use crate::operations::infrastructure::k8s::managed_service::KubernetesRenderer;
use std::sync::Arc;

impl Default for InstanceService {
    fn default() -> Self {
        Self::new(Arc::new(KubernetesRenderer))
    }
}
