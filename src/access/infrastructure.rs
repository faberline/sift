//! Where credentials come from: the auth configuration read from the
//! environment and the token registry, and the verifier that checks a bearer
//! token against a static role map or the Kubernetes API server.

pub(crate) mod auth_config;
pub(crate) mod sift_verifier;
