//! The MCP servers: standard input, and HTTP with its allowed hosts and
//! origins.

use std::time::Duration;

use anyhow::Result;

use crate::query::infrastructure::sift_api_client::SiftApiClient;
use crate::query::interfaces::mcp::sift_mcp_server::SiftMcpServer;

const MCP_TIMEOUT: Duration = Duration::from_secs(30);
const MCP_ALLOWED_HOSTS_ENV: &str = "SIFT_MCP_ALLOWED_HOSTS";
const MCP_ALLOWED_ORIGINS_ENV: &str = "SIFT_MCP_ALLOWED_ORIGINS";

/// Serve the five read-only tools over MCP standard input and output.
pub async fn serve_stdio(endpoint: String, token: Option<String>) -> Result<()> {
    let server = SiftMcpServer::new(SiftApiClient::new(&endpoint, token, MCP_TIMEOUT)?);
    service_mcp::serve_stdio(server).await
}

/// Build the official Streamable HTTP transport at `/mcp`.
pub fn http_router(endpoint: &str) -> Result<axum::Router> {
    let base = SiftApiClient::new(endpoint, None, MCP_TIMEOUT)?;
    let default_hosts = ["localhost", "127.0.0.1", "::1"]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let default_origins = vec![base.endpoint.origin().ascii_serialization()];
    let config = service_mcp::HttpTransportConfig::from_env(
        MCP_ALLOWED_HOSTS_ENV,
        MCP_ALLOWED_ORIGINS_ENV,
        default_hosts,
        default_origins,
    )?;
    Ok(service_mcp::streamable_http_router(
        "/mcp",
        SiftMcpServer::new(base),
        config,
    ))
}
