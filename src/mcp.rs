//! The MCP tools' public paths. The query context now owns the read-only
//! MCP tools and the Sift API client behind them; these re-exports keep
//! `sift::mcp::*` compiling for callers outside the crate.

pub use crate::query::infrastructure::sift_api_client::SiftApiClient;
pub use crate::query::interfaces::mcp::mcp_transport::{http_router, serve_stdio};
pub use crate::query::interfaces::mcp::sift_mcp_server::tool_names;
