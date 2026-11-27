//! Select the Kubernetes implementation of the instance rendering port.

use crate::operations::application::instance_service::{
    precreated_auth_delegator_from, InstanceService, PRECREATED_AUTH_DELEGATOR_ENV,
};
use crate::operations::infrastructure::k8s::managed_service::KubernetesRenderer;
use std::sync::Arc;

impl Default for InstanceService {
    fn default() -> Self {
        Self::new(Arc::new(KubernetesRenderer)).with_precreated_auth_delegator(
            precreated_auth_delegator_from(
                std::env::var(PRECREATED_AUTH_DELEGATOR_ENV).ok().as_deref(),
            ),
        )
    }
}
