//! Snapshot framing and staging keep filesystem work outside application.

use crate::journal::domain::journal_api::JournalApi;
use crate::shared_kernel::snapshot_metadata::SnapshotMetadata;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::Arc;

pub(crate) trait ReadSeek: Read + Seek {}
impl<T: Read + Seek + ?Sized> ReadSeek for T {}

pub(crate) trait SnapshotCodec: Send + Sync {
    type Error;
    fn write(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
        applied_index: u64,
        writer: &mut dyn Write,
    ) -> Result<SnapshotMetadata, Self::Error>;
    fn restore(
        &self,
        journal: &dyn JournalApi<Error = Self::Error>,
        reader: &mut dyn ReadSeek,
    ) -> Result<SnapshotMetadata, Self::Error>;
}
pub(crate) trait JournalStaging: Send + Sync {
    type Error;
    fn open(&self, root: &Path) -> Result<Arc<dyn JournalApi<Error = Self::Error>>, Self::Error>;
}
