//! Sealing the journal prefix for the archive, and compacting the archived WAL.

use anyhow::Result;

use crate::event::SignalKind;
use crate::journal::infrastructure::durable_journal::DurableJournal;

impl DurableJournal {
    /// Seal one globally consistent archive prefix.
    ///
    /// Ingest holds this same journal-state write lock while it allocates a
    /// cursor, writes the canonical WAL, and appends rebuildable segments. The
    /// archive therefore cannot observe a later signal while missing an
    /// earlier cursor from another signal.
    pub(crate) fn seal_archive_prefix(
        &self,
    ) -> Result<(u64, Vec<(SignalKind, crate::storage::SegmentManifest)>)> {
        let state = self.state.write().expect("journal state lock poisoned");
        let captured_cursor = state.last_cursor;
        let segments = self.storage.seal_all_with_signal()?;
        drop(state);
        Ok((captured_cursor, segments))
    }

    pub(crate) fn compact_archived_wal(
        &self,
        watermarks: crate::shared_kernel::archive_watermarks::ArchiveWatermarks,
    ) -> Result<()> {
        self.wal.compact_through(watermarks)
    }
}
