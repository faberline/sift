//! The names, API group, component labels and ports every rendered Sift object
//! shares.

pub(super) const APP: &str = "sift";
pub(super) const API_VERSION: &str = "sift.axiom.dev/v1alpha1";
pub(super) const KIND: &str = "Sift";
pub(super) const GATEWAY_COMPONENT: &str = "gateway";
pub(super) const QUERY_COMPONENT: &str = "query";
pub(super) const STORE_COMPONENT: &str = "store";
pub(super) const CONTROL_COMPONENT: &str = "control";
pub(super) const AGENT_COMPONENT: &str = "agent";
pub(super) const BACKUP_COMPONENT: &str = "backup";
pub(super) const AUTH_DELEGATION_COMPONENT: &str = "auth-delegation";
pub(super) const AUTH_DELEGATOR_ROLE: &str = "system:auth-delegator";
pub(super) const HTTP_PORT: i32 = 7380;
pub(super) const OTLP_GRPC_PORT: i32 = 4317;
pub(super) const PEER_MTLS_PORT: i32 = 7381;
