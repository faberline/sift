//! Loading and atomically persisting the journal head control record.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::journal::domain::journal_head::{JournalHead, JOURNAL_HEAD_FORMAT_VERSION};

const JOURNAL_HEAD_PATH: &str = "control/journal-head.json";

impl JournalHead {
    pub fn load(root: &Path) -> Result<Option<Self>> {
        let path = root.join(JOURNAL_HEAD_PATH);
        if !path.exists() {
            return Ok(None);
        }
        let head: Self = serde_json::from_slice(
            &fs::read(&path).with_context(|| format!("read journal head {}", path.display()))?,
        )
        .with_context(|| format!("decode journal head {}", path.display()))?;
        if head.format_version != JOURNAL_HEAD_FORMAT_VERSION {
            bail!(
                "unsupported journal head format {}; expected {}",
                head.format_version,
                JOURNAL_HEAD_FORMAT_VERSION
            );
        }
        if head.retained_events > head.last_cursor {
            bail!("journal head retained event count exceeds its last cursor");
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        Ok(Some(head))
    }

    pub fn persist(self, root: &Path) -> Result<()> {
        if self.retained_events > self.last_cursor {
            bail!("journal head retained event count exceeds its last cursor");
        }
        let path = root.join(JOURNAL_HEAD_PATH);
        storage_durable::atomic_write(
            &path,
            &serde_json::to_vec_pretty(&self)?,
            storage_durable::FsyncPolicy::Always,
        )
        .with_context(|| format!("persist journal head {}", path.display()))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }
}
