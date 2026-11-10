//! Sift's HTTP data plane: the router that mounts every context's handlers, the
//! error every handler returns, and the OpenAPI document.

pub(crate) mod api_error;
pub(crate) mod control_records;
pub(crate) mod openapi;
mod production_schema;
pub(crate) mod recovery;
pub(crate) mod router;
mod schemas;
