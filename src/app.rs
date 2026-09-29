//! The assembly root: the service state every context's handlers share, the
//! readiness and metrics hooks service_http reads, and the HTTP data plane with
//! its API error and OpenAPI document. The app wires the contexts together and
//! owns no domain rules of its own.

pub(crate) mod interfaces;
pub(crate) mod service_state;
