//! `sift backup`: ships a live or explicitly offline journal snapshot through the
//! shared backup contract.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use sift::DurableJournal;

use crate::terminal_output::print_json_terminal;

#[derive(Args)]
pub(super) struct BackupArgs {
    /// Running Sift base URL. Fetches the protected /admin/backup snapshot.
    #[arg(
        long,
        env = "SIFT_BACKUP_URL",
        conflicts_with = "data_dir",
        required_unless_present = "data_dir"
    )]
    url: Option<String>,
    /// Legacy offline mode: open this stopped journal directly.
    #[arg(long, conflicts_with = "url", required_unless_present = "url")]
    data_dir: Option<PathBuf>,
    /// Admin bearer token for live mode. Invalid with offline --data-dir.
    #[arg(
        long,
        env = "SIFT_BACKUP_TOKEN",
        requires = "url",
        conflicts_with = "token_file"
    )]
    token: Option<String>,
    /// Rotating projected ServiceAccount token for live mode.
    #[arg(
        long,
        env = "SIFT_TOKEN_FILE",
        requires = "url",
        conflicts_with = "token"
    )]
    token_file: Option<PathBuf>,
    #[arg(long, env = "SIFT_TOKEN_AUDIENCE", default_value = "sift.axiom.dev")]
    token_audience: String,
    /// Project checked by Kubernetes SubjectAccessReview for live backup.
    #[arg(long, env = "SIFT_PROJECT", default_value = "*")]
    project: String,
    /// Shared backup destination URI: file://, s3://, or gs://.
    #[arg(long)]
    dest: String,
    /// Remove backup objects older than this many seconds after a successful write.
    #[arg(long)]
    retention_secs: Option<u64>,
}

pub(super) async fn backup(args: BackupArgs) -> Result<()> {
    let result = match (args.url.as_deref(), args.data_dir.as_deref()) {
        (Some(url), None) => {
            sift::backup::backup_live_journal_authenticated(
                url,
                args.token.as_deref(),
                args.token_file.as_deref(),
                &args.token_audience,
                &args.project,
                &args.dest,
                args.retention_secs,
            )
            .await?
        }
        (None, Some(data_dir)) => {
            let journal = DurableJournal::open(data_dir)?;
            sift::backup::backup_journal(&journal, &args.dest, args.retention_secs)?
        }
        _ => anyhow::bail!(
            "choose exactly one backup source: live --url or legacy offline --data-dir"
        ),
    };
    print_json_terminal(result)
}
