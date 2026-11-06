//! The original snapshot framing behind the journal codec port.
use crate::journal::domain::journal_api::JournalApi;
use crate::journal::domain::snapshot_ports::{ReadSeek, SnapshotCodec};
use crate::shared_kernel::snapshot_metadata::SnapshotMetadata;
use anyhow::Result;
use std::io::Write;
pub(crate) struct JournalSnapshotCodec;
impl SnapshotCodec for JournalSnapshotCodec {
    type Error = anyhow::Error;
    fn write(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
        applied_index: u64,
        writer: &mut dyn Write,
    ) -> Result<SnapshotMetadata> {
        super::raft::snapshot_writer::write_snapshot(journal, applied_index, writer)
    }
    fn restore(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
        reader: &mut dyn ReadSeek,
    ) -> Result<SnapshotMetadata> {
        super::raft::snapshot_restore::restore_seekable_snapshot(journal, reader)
    }
}
