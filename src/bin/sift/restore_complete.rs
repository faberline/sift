//! Restore the data prefix and its matching control state.
use anyhow::Result;
use clap::Args;
use std::path::PathBuf;
#[derive(Args)]
pub(super) struct RestoreCompleteArgs {
    /// GCS archive prefix that contains recovery/latest.json.
    #[arg(long)]
    source: String,
    /// Empty store PVC mount or offline store directory.
    #[arg(long)]
    store_dir: PathBuf,
    /// Empty control PVC mount or offline control directory.
    #[arg(long)]
    control_dir: PathBuf,
    /// The same saved recovery record must be used for every replacement PVC.
    #[arg(long)]
    record_file: Option<PathBuf>,
}
pub(super) async fn run(args: RestoreCompleteArgs) -> Result<()> {
    let record = tokio::task::spawn_blocking(move || {
        sift::operations::restore_complete_recovery_pinned(
            &args.source,
            &args.store_dir,
            &args.control_dir,
            args.record_file.as_deref(),
        )
    })
    .await??;
    crate::terminal_output::print_json_terminal(serde_json::to_value(record)?)
}
