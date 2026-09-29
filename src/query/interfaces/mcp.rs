//! Read-only Model Context Protocol tools backed by the public Sift API.
//!
//! The standard-input server and the HTTP server use the same tool handler.
//! HTTP requests forward their bearer token to the normal Sift API. This keeps
//! project authorization in one place.

pub(crate) mod mcp_transport;
pub(crate) mod sift_mcp_server;
