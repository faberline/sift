//! `sift collect`: collects axiom.service.log.v1 JSONL from a file, stdin, or
//! Kubernetes CRI logs, keeping each source's checkpoint under the data root's
//! agent directory.

use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use clap::Args;
use sha2::{Digest, Sha256};
use sift::collector::{
    CollectorConfig, CriMetadata, CriSourceConfig, SourceSpec, DEFAULT_BATCH_SIZE,
    DEFAULT_MAX_LINE_BYTES, DEFAULT_MAX_RETRIES,
};

use crate::terminal_output::print_json_terminal;

#[derive(Args)]
pub(super) struct CollectArgs {
    /// JSONL source path, or `-` for stdin.
    #[arg(
        long,
        conflicts_with = "cri_root",
        required_unless_present = "cri_root"
    )]
    source: Option<String>,
    /// Kubernetes node CRI pod-log root, normally /var/log/pods.
    #[arg(long, conflicts_with = "source", required_unless_present = "source")]
    cri_root: Option<PathBuf>,
    /// Stable identity used with byte offsets to derive idempotency keys.
    #[arg(long)]
    source_id: Option<String>,
    /// Sift service base URL.
    #[arg(long, env = "SIFT_URL", default_value = "http://127.0.0.1:7380")]
    endpoint: String,
    /// Optional Sift bearer token.
    #[arg(long, env = "SIFT_TOKEN", conflicts_with = "token_file")]
    token: Option<String>,
    /// Rotating projected ServiceAccount token. The file is read for every request.
    #[arg(long, env = "SIFT_TOKEN_FILE", conflicts_with = "token")]
    token_file: Option<PathBuf>,
    /// Required audience in a projected ServiceAccount token.
    #[arg(long, env = "SIFT_TOKEN_AUDIENCE", default_value = "sift.axiom.dev")]
    token_audience: String,
    /// Persistent Sift root used for agent checkpoints and rejected records.
    #[arg(long, env = "SIFT_DATA_DIR", default_value = sift::storage::DEFAULT_DATA_DIR)]
    data_dir: PathBuf,
    #[arg(long, default_value = "default")]
    project: String,
    #[arg(long, default_value = "local")]
    environment: String,
    /// GCP project used for k8s_container monitored-resource identity.
    #[arg(long, env = "GCP_PROJECT_ID")]
    gcp_project: Option<String>,
    #[arg(long, env = "GKE_CLUSTER_NAME")]
    cluster: Option<String>,
    #[arg(long, env = "GKE_LOCATION")]
    location: Option<String>,
    #[arg(long, env = "NODE_NAME")]
    node: Option<String>,
    /// Durable source offset checkpoint; defaults under DATA_DIR/agent.
    #[arg(long)]
    checkpoint: Option<PathBuf>,
    /// Invalid-line JSONL sink; defaults beside the checkpoint.
    #[arg(long)]
    quarantine: Option<PathBuf>,
    #[arg(long, default_value_t = DEFAULT_BATCH_SIZE)]
    batch_size: usize,
    #[arg(long, default_value_t = DEFAULT_MAX_LINE_BYTES)]
    max_line_bytes: usize,
    #[arg(long, default_value_t = DEFAULT_MAX_RETRIES)]
    max_retries: usize,
    #[arg(long, default_value_t = 10)]
    request_timeout_secs: u64,
    /// Continue watching a regular file or refreshing CRI discovery after EOF.
    #[arg(long)]
    follow: bool,
    #[arg(long, default_value_t = 250)]
    follow_poll_ms: u64,
}

pub(super) async fn collect(args: CollectArgs) -> Result<()> {
    let (source, default_source_id) = if let Some(root) = args.cri_root {
        let canonical = std::fs::canonicalize(&root)
            .with_context(|| format!("resolve CRI root {}", root.display()))?;
        let gcp_project = args
            .gcp_project
            .context("--gcp-project or GCP_PROJECT_ID is required with --cri-root")?;
        let source_id = args
            .node
            .as_deref()
            .map(|node| format!("cri-node:{node}"))
            .unwrap_or_else(|| format!("cri-root:{}", canonical.display()));
        (
            SourceSpec::Cri(CriSourceConfig {
                root: canonical,
                metadata: CriMetadata {
                    gcp_project,
                    cluster: args.cluster,
                    location: args.location,
                    node: args.node,
                },
            }),
            source_id,
        )
    } else if args.source.as_deref() == Some("-") {
        (SourceSpec::Stdin, "stdin".to_string())
    } else {
        let source_arg = args.source.context("--source or --cri-root is required")?;
        let path = PathBuf::from(&source_arg);
        let canonical = std::fs::canonicalize(&path)
            .with_context(|| format!("resolve collector source {}", path.display()))?;
        let source_id = format!("file:{}", canonical.display());
        (SourceSpec::File(path), source_id)
    };
    let source_id = args.source_id.unwrap_or(default_source_id);
    let checkpoint_path = args
        .checkpoint
        .unwrap_or_else(|| agent_checkpoint_path(&args.data_dir, &source_id));
    let quarantine_path = args
        .quarantine
        .unwrap_or_else(|| PathBuf::from(format!("{}.rejected.jsonl", checkpoint_path.display())));
    let summary = sift::collector::run_collector(CollectorConfig {
        source,
        source_id,
        endpoint: args.endpoint,
        token: args.token,
        token_file: args.token_file,
        token_audience: args.token_audience,
        project: args.project,
        environment: args.environment,
        checkpoint_path,
        quarantine_path,
        batch_size: args.batch_size,
        max_line_bytes: args.max_line_bytes,
        max_retries: args.max_retries,
        request_timeout: Duration::from_secs(args.request_timeout_secs),
        initial_backoff: Duration::from_millis(50),
        follow: args.follow,
        follow_poll_interval: Duration::from_millis(args.follow_poll_ms),
    })
    .await?;
    print_json_terminal(summary)
}

fn agent_checkpoint_path(data_dir: &std::path::Path, source_id: &str) -> PathBuf {
    let digest = Sha256::digest(source_id.as_bytes());
    data_dir
        .join("agent")
        .join(format!("{}.checkpoint.json", hex::encode(&digest[..16])))
}
