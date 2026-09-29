//! Where operations meets the outside: the manifests rendered from checked-in
//! templates, the client that fetches a live snapshot from a running service,
//! and the Kubernetes objects the operator renders.

pub(crate) mod k8s;
pub(crate) mod live_snapshot_client;
pub(crate) mod manifest_bundle;
