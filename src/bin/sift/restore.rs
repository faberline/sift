//! `sift restore`: restores a journal snapshot from a shared backup object URI.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use sift::DurableJournal;

use crate::terminal_output::print_json_terminal;

#[derive(Args)]
pub(super) struct RestoreArgs {
    #[arg(long, env = "SIFT_DATA_DIR", default_value = sift::storage::DEFAULT_DATA_DIR)]
    data_dir: PathBuf,
    /// Source URI accepted by service-backup, for example file:///backup/sift.json.
    #[arg(long)]
    source: String,
}

pub(super) fn restore(args: RestoreArgs) -> Result<()> {
    let journal = DurableJournal::open(&args.data_dir)?;
    sift::backup::restore_journal(&journal, &args.source)?;
    print_json_terminal(serde_json::json!({
        "status": "restored",
        "source": args.source,
    }))
}
