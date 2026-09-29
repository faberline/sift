//! The operator loop: service_k8s's shared leader-elected reconcile loop over
//! the Sift custom resource.

use crate::operations::interfaces::crd::sift_spec::Sift;

pub async fn run() -> anyhow::Result<()> {
    service_k8s::run::<Sift>().await
}
