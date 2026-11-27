//! The operator loop: service_k8s's shared leader-elected reconcile loop over
//! the Sift custom resource. The watch is cluster-wide unless the environment
//! variable `WATCH_NAMESPACES` lists namespaces (a comma list).

use crate::operations::interfaces::crd::sift_spec::Sift;

/// The environment variable that limits the watch to a comma list of
/// namespaces. Unset or empty means a cluster-wide watch.
pub const WATCH_NAMESPACES_ENV: &str = "WATCH_NAMESPACES";

pub async fn run() -> anyhow::Result<()> {
    service_k8s::run_in_namespaces::<Sift>(service_k8s::WatchScope::from_env(WATCH_NAMESPACES_ENV))
        .await
}
