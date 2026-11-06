//! The names, API group, component labels and ports every rendered Sift object
//! shares.

pub(in crate::operations) const APP: &str = "sift";
pub(in crate::operations) const API_VERSION: &str = "sift.axiom.dev/v1alpha1";
pub(in crate::operations) const KIND: &str = "Sift";
pub(in crate::operations) const GATEWAY_COMPONENT: &str = "gateway";
pub(in crate::operations) const QUERY_COMPONENT: &str = "query";
pub(in crate::operations) const STORE_COMPONENT: &str = "store";
pub(in crate::operations) const CONTROL_COMPONENT: &str = "control";
pub(in crate::operations) const AGENT_COMPONENT: &str = "agent";
pub(in crate::operations) const BACKUP_COMPONENT: &str = "backup";
pub(in crate::operations) const AUTH_DELEGATION_COMPONENT: &str = "auth-delegation";
pub(in crate::operations) const AUTH_DELEGATOR_ROLE: &str = "system:auth-delegator";
pub(in crate::operations) const HTTP_PORT: i32 = 7380;
pub(in crate::operations) const OTLP_GRPC_PORT: i32 = 4317;
pub(in crate::operations) const PEER_MTLS_PORT: i32 = 7381;
