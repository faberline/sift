//! The assembly root: the service state every context's handlers share, the
//! readiness and metrics hooks service_http reads, the HTTP data plane with
//! its API error and OpenAPI document, and the event constructors that read
//! the system clock. The app wires the contexts together and owns no domain
//! rules of its own.

mod access_wiring;
mod archive_wiring;
pub(crate) mod collector_wiring;
pub(crate) mod grpc_authorization;
mod instance_wiring;
pub(crate) mod interfaces;
pub(crate) mod otlp_wiring;
pub(crate) mod service_state;
pub(crate) mod system_clock;

mod journal_command_wiring;

mod journal_storage_wiring;

mod projection_wiring;

mod query_error_wiring;

pub(crate) mod query_wiring;
