//! Sift's HTTP data plane: the router that mounts every context's handlers, the
//! error every handler returns, and the OpenAPI document.

pub(crate) mod api_error;
pub(crate) mod openapi;
pub(crate) mod router;
