//! The assembly root: the service state every context's handlers share, the
//! readiness and metrics hooks service_http reads, the HTTP data plane with
//! its API error and OpenAPI document, and the event constructors that read
//! the system clock. The app wires the contexts together and owns no domain
//! rules of its own.

pub(crate) mod interfaces;
pub(crate) mod service_state;
pub(crate) mod system_clock;
