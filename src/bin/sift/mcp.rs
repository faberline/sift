//! `sift mcp serve`: serves the read-only Sift MCP tools over standard input and
//! output, against a Sift endpoint.

use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub(super) struct McpArgs {
    #[command(subcommand)]
    command: McpCommand,
}

#[derive(Subcommand)]
enum McpCommand {
    /// Serve Sift tools over standard input and output.
    Serve(McpServeArgs),
}

#[derive(Args)]
struct McpServeArgs {
    /// Use the MCP standard-input and standard-output transport.
    #[arg(long, default_value_t = true, action = clap::ArgAction::SetTrue)]
    stdio: bool,
    /// Sift service base URL used by the MCP tools.
    #[arg(long, env = "SIFT_URL", default_value = "http://127.0.0.1:7380")]
    endpoint: String,
    /// Optional Sift bearer token.
    #[arg(long, env = "SIFT_TOKEN")]
    token: Option<String>,
}

pub(super) async fn mcp(args: McpArgs) -> Result<()> {
    match args.command {
        McpCommand::Serve(args) => {
            if !args.stdio {
                anyhow::bail!("only --stdio is supported by `sift mcp serve`");
            }
            sift::mcp::serve_stdio(args.endpoint, args.token).await
        }
    }
}
