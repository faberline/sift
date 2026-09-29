//! Operations: running Sift as a service. It owns the Kubernetes operator and
//! the Sift custom resource it reconciles, the deployment artifacts rendered
//! offline from checked-in templates, off-node snapshot backup and restore, the
//! gateway's reverse-proxy route policy, the OTLP/gRPC proxy, and the admin
//! endpoints that export a snapshot or verify one project's integrity.

pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;
