//! The journal as snapshot bytes, and back.

use anyhow::{Context, Result};

use crate::journal::application::journal_runtime::JournalRuntime;

impl JournalRuntime {
    pub fn snapshot_bytes(&self) -> Result<Vec<u8>> {
        let mut snapshot = Vec::new();
        self.snapshots
            .write(self, self.last_cursor(), &mut snapshot)
            .context("serialize durable journal snapshot")?;
        Ok(snapshot)
    }

    pub fn restore_snapshot_bytes(&self, bytes: &[u8]) -> Result<()> {
        let mut cursor = std::io::Cursor::new(bytes);
        self.snapshots
            .restore(self, &mut cursor)
            .context("restore durable journal snapshot")?;
        Ok(())
    }
}
