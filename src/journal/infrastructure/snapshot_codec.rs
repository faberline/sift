//! The journal as snapshot bytes, and back.

use anyhow::{Context, Result};

use crate::journal::infrastructure::durable_journal::DurableJournal;

impl DurableJournal {
    pub fn snapshot_bytes(&self) -> Result<Vec<u8>> {
        let mut snapshot = Vec::new();
        crate::journal::infrastructure::raft::snapshot_writer::write_snapshot(
            self,
            self.last_cursor(),
            &mut snapshot,
        )
        .context("serialize durable journal snapshot")?;
        Ok(snapshot)
    }

    pub fn restore_snapshot_bytes(&self, bytes: &[u8]) -> Result<()> {
        let mut cursor = std::io::Cursor::new(bytes);
        crate::journal::infrastructure::raft::snapshot_restore::restore_seekable_snapshot(
            self,
            &mut cursor,
        )
        .context("restore durable journal snapshot")?;
        Ok(())
    }
}
