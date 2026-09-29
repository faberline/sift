//! `sift snapshot`: writes a consistent Sift journal snapshot to stdout or a
//! local file.

use std::path::PathBuf;

use anyhow::{Context, Result};
use base64::Engine as _;
use clap::Args;
use sift::DurableJournal;

use crate::terminal_output::print_json_terminal;

#[derive(Args)]
pub(super) struct SnapshotArgs {
    #[arg(long, env = "SIFT_DATA_DIR", default_value = sift::storage::DEFAULT_DATA_DIR)]
    data_dir: PathBuf,
    /// Write the raw snapshot bytes here; omit to emit them in a JSON terminal envelope.
    #[arg(long)]
    out: Option<PathBuf>,
}

pub(super) fn snapshot(args: SnapshotArgs) -> Result<()> {
    let bytes = DurableJournal::open(&args.data_dir)?.snapshot_bytes()?;
    if let Some(path) = args.out {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("create snapshot output directory {}", parent.display())
            })?;
        }
        std::fs::write(&path, &bytes)
            .with_context(|| format!("write Sift snapshot {}", path.display()))?;
        return print_json_terminal(serde_json::json!({
            "path": path,
            "bytes": bytes.len(),
        }));
    }
    print_json_terminal(serde_json::json!({
        "format": "sift-snapshot-v2",
        "encoding": "base64",
        "bytes": bytes.len(),
        "snapshot_base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
    }))
}
