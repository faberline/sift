//! Access: who may call Sift, and for which project. Local deployments may use
//! a static role map; GKE delegates identity and project authorization to the
//! Kubernetes API server.
//!
//! `SIFT_AUTH=required` protects only data-plane routes. Standard health,
//! readiness, metrics, OpenAPI, and docs routes remain operable for platform
//! probes. Production tokens are read from `SIFT_TOKEN_REGISTRY_FILE`; the
//! inline `SIFT_TOKENS` variable is retained for local development.

pub(crate) mod application;
pub(crate) mod domain;
pub(crate) mod infrastructure;
pub(crate) mod interfaces;

pub use application::authorization_service::AuthorizationService;

pub use crate::access::{
    infrastructure::{auth_config::SiftAuthConfig, sift_verifier::SiftVerifier},
    interfaces::http::scoped_authorization::auth_middleware,
};
