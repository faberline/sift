//! `sift serve`'s arguments: the internal deployment role, the listeners, the
//! data root or ephemeral storage, logging, the drain grace, the body limit and
//! the OTLP endpoint.

use std::path::PathBuf;

use clap::{Args, ValueEnum};

#[derive(Args)]
pub(super) struct ServeArgs {
    /// Internal Sift deployment role. All roles use the same product binary.
    #[arg(long, value_enum, default_value = "all")]
    pub(super) role: RunRole,
    #[arg(long, env = "SIFT_HOST", default_value = "0.0.0.0")]
    pub(super) host: String,
    #[arg(long, env = "SIFT_PORT", default_value_t = 7380)]
    pub(super) port: u16,
    /// OTLP/gRPC listener port. It defaults to 4317 for the normal HTTP port.
    #[arg(long, env = "SIFT_GRPC_PORT")]
    pub(super) grpc_port: Option<u16>,
    #[arg(long, env = "SIFT_DATA_DIR", default_value = sift::storage::DEFAULT_DATA_DIR)]
    pub(super) data_dir: PathBuf,
    /// Development-only temporary storage. Production roles refuse this flag.
    #[arg(long, conflicts_with = "data_dir")]
    pub(super) ephemeral: bool,
    #[arg(long, env = "SIFT_LOG_LEVEL", default_value = "info")]
    pub(super) log_level: String,
    #[arg(long, env = "SIFT_LOG_FORMAT", value_enum, default_value_t = LogFormat::Json)]
    pub(super) log_format: LogFormat,
    #[arg(long, env = "SIFT_GRACE_SECS", default_value_t = 20)]
    pub(super) grace_secs: u64,
    #[arg(long, env = "SIFT_MAX_BODY_BYTES", default_value_t = 1_048_576)]
    pub(super) max_body_bytes: usize,
    #[arg(long, env = "SIFT_OTLP_ENDPOINT")]
    pub(super) otlp_endpoint: Option<String>,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum LogFormat {
    Pretty,
    Json,
}

#[derive(Clone, Copy, Eq, PartialEq, ValueEnum)]
pub(super) enum RunRole {
    All,
    Agent,
    Gateway,
    Query,
    Store,
    Control,
    Operator,
}

impl From<RunRole> for sift::storage::StorageRole {
    fn from(role: RunRole) -> Self {
        match role {
            RunRole::All => Self::All,
            RunRole::Agent => Self::Agent,
            RunRole::Gateway => Self::Gateway,
            RunRole::Query => Self::Query,
            RunRole::Store => Self::Store,
            RunRole::Control => Self::Control,
            RunRole::Operator => Self::Operator,
        }
    }
}
