//! `sift connect`: runs a command through a managed Kubernetes port-forward to
//! Sift, resolving the Service and its token secret.

use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};

/// Manage a `kubectl port-forward` around a wrapped command so callers do not
/// have to track the child process or hand-resolve the token registry secret.
#[derive(Args)]
pub(super) struct ConnectArgs {
    /// kubectl context to port-forward through; omit to use the current context.
    #[arg(long)]
    context: Option<String>,
    /// Namespace of the target Service or Sift custom resource.
    #[arg(long)]
    namespace: String,
    /// Target Service name; defaults to `--cr` when a custom resource is named.
    #[arg(long)]
    service: Option<String>,
    /// Sift custom-resource name used to discover the target Service and token Secret.
    #[arg(long)]
    cr: Option<String>,
    /// Local port to forward to; omit to allocate an ephemeral port.
    #[arg(long)]
    local_port: Option<u16>,
    /// Remote Service port.
    #[arg(long, default_value_t = 7380)]
    remote_port: u16,
    /// Token-registry Secret name. Auto-discovered from `--cr` when omitted.
    #[arg(long)]
    secret: Option<String>,
    /// Explicit bearer token; otherwise one is selected from the token registry.
    #[arg(long, env = "SIFT_TOKEN")]
    token: Option<String>,
    /// Minimum role required of a token selected from the registry.
    #[arg(long, value_enum, default_value_t = TokenRole::Admin)]
    role: TokenRole,
    /// Optional resource scope used while selecting a registry token.
    #[arg(long)]
    resource: Option<String>,
    /// Command to run with `SIFT_URL` and, when available, `SIFT_TOKEN` set.
    #[arg(last = true, required = true)]
    command: Vec<String>,
}

/// Service-owned mapping to the generic `cli-std` token-role hierarchy.
#[derive(Clone, Copy, ValueEnum)]
enum TokenRole {
    Read,
    Write,
    Admin,
}

impl From<TokenRole> for cli_std::connect::Role {
    fn from(role: TokenRole) -> Self {
        match role {
            TokenRole::Read => Self::Read,
            TokenRole::Write => Self::Write,
            TokenRole::Admin => Self::Admin,
        }
    }
}

/// Shared k8s-native connection lifecycle: resolve the Service and optional
/// token registry, wait for a ready local port-forward, run the wrapped
/// command, then let `ChildGuard` terminate and reap kubectl on every exit.
pub(super) async fn connect(args: ConnectArgs) -> Result<()> {
    let service = args
        .service
        .clone()
        .or_else(|| args.cr.clone())
        .context("--service or --cr is required")?;
    let secret = match args.secret.clone() {
        Some(secret) => Some(secret),
        None => match &args.cr {
            Some(cr) => cli_std::connect::resolve_cr_tokens_secret(
                args.context.as_deref(),
                &args.namespace,
                "sift",
                cr,
            )?,
            None => None,
        },
    };
    let local_port = args
        .local_port
        .map(Ok)
        .unwrap_or_else(cli_std::connect::free_local_port)?;

    let mut forward = std::process::Command::new("kubectl");
    if let Some(context) = &args.context {
        forward.args(["--context", context]);
    }
    forward.args([
        "port-forward",
        "-n",
        &args.namespace,
        &format!("svc/{service}"),
        &format!("{local_port}:{}", args.remote_port),
    ]);
    forward.stdout(std::process::Stdio::null());
    forward.stderr(std::process::Stdio::null());
    let _forward =
        cli_std::connect::ChildGuard::spawn(&mut forward).context("start kubectl port-forward")?;
    cli_std::connect::wait_for_local_port_ready(local_port, Duration::from_secs(30))?;

    let token = cli_std::connect::resolve_token(
        args.token.as_deref(),
        args.context.as_deref(),
        Some(&args.namespace),
        secret.as_deref(),
        args.role.into(),
        args.resource.as_deref(),
    )?;
    let (program, rest) = args
        .command
        .split_first()
        .context("wrapped command is empty")?;
    let mut command = std::process::Command::new(program);
    command.args(rest);
    command.env("SIFT_URL", format!("http://127.0.0.1:{local_port}"));
    if let Some(token) = token {
        command.env("SIFT_TOKEN", token);
    }
    let status = command.status().context("run wrapped command")?;
    if !status.success() {
        anyhow::bail!("wrapped command exited with {status}");
    }
    Ok(())
}
