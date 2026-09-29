//! The proxy module's public paths. The operations context now owns the
//! gateway's reverse-proxy route policy; these re-exports keep
//! `sift::proxy::*` compiling for callers outside the crate.

pub use crate::operations::interfaces::http::gateway_proxy::{gateway_router, query_router};
