//! Run the native node-local metric collector.
use anyhow::Result;
use clap::Args;
use std::{path::PathBuf, time::Duration};
#[derive(Args)]
pub(super) struct ScrapeArgs {
    #[arg(long, env = "SIFT_URL", default_value = "http://127.0.0.1:7380")]
    endpoint: String,
    #[arg(long)]
    project: String,
    #[arg(long, default_value = "production")]
    environment: String,
    #[arg(long, env = "NODE_NAME")]
    node: String,
    /// Restrict discovery to this namespace. Default: all namespaces.
    #[arg(long)]
    namespace: Option<String>,
    #[arg(long, env = "SIFT_TOKEN", conflicts_with = "token_file")]
    token: Option<String>,
    #[arg(long, env = "SIFT_TOKEN_FILE", conflicts_with = "token")]
    token_file: Option<PathBuf>,
    #[arg(long, default_value = "/var/lib/sift/agent/metrics")]
    state: PathBuf,
    #[arg(long, default_value_t = 30)]
    interval_seconds: u64,
    #[arg(long)]
    once: bool,
}
pub(super) async fn scrape(args: ScrapeArgs) -> Result<()> {
    sift::collector::run_metric_scraper(sift::collector::ScrapeConfig {
        endpoint: args.endpoint,
        project: args.project,
        environment: args.environment,
        node: args.node,
        namespace: args.namespace,
        token: args.token,
        token_file: args.token_file,
        state: args.state,
        interval: Duration::from_secs(args.interval_seconds),
        once: args.once,
    })
    .await
}
