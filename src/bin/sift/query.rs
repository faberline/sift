//! `sift query`: runs one versioned logs, metrics, or traces query through the
//! Sift API, reading the request from a file or stdin.

use std::{io::Read, time::Duration};

use anyhow::{Context, Result};
use clap::Args;

use crate::terminal_output::print_json_terminal;

#[derive(Args)]
pub(super) struct QueryArgs {
    /// V1 or V2 query JSON file, or `-` for stdin.
    #[arg(value_name = "REQUEST")]
    request: String,
    /// Sift service base URL.
    #[arg(long, env = "SIFT_URL", default_value = "http://127.0.0.1:7380")]
    endpoint: String,
    /// Optional Sift bearer token.
    #[arg(long, env = "SIFT_TOKEN")]
    token: Option<String>,
    #[arg(long, default_value_t = 30)]
    request_timeout_secs: u64,
}

pub(super) async fn query(args: QueryArgs) -> Result<()> {
    let source = read_json_input(&args.request)?;
    let value: serde_json::Value = serde_json::from_slice(&source).context("parse query JSON")?;
    let api = sift::query::SiftApiClient::new(
        &args.endpoint,
        args.token,
        Duration::from_secs(args.request_timeout_secs),
    )?;
    if value.get("version").and_then(serde_json::Value::as_u64) == Some(2) {
        let request: sift::query::QueryRequestV2 =
            serde_json::from_value(value).context("parse QueryRequestV2")?;
        print_json_terminal(api.query_v2(&request).await?)
    } else {
        let request: sift::query::QueryRequestV1 =
            serde_json::from_value(value).context("parse QueryRequestV1")?;
        request.validate().context("validate QueryRequestV1")?;
        print_json_terminal(api.query(&request).await?)
    }
}

fn read_json_input(source: &str) -> Result<Vec<u8>> {
    if source == "-" {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .context("read JSON from stdin")?;
        return Ok(bytes);
    }
    std::fs::read(source).with_context(|| format!("read JSON file {source}"))
}
