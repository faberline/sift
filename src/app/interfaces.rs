//! How the service is reached: the readiness and metrics hooks service_http
//! calls, and the HTTP data plane.

pub(crate) mod http;
pub(crate) mod metrics_provider;
pub(crate) mod readiness_hook;
