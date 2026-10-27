//! Resources opened at the original cold-restore I/O points.

use crate::shared_kernel::event::ContentBlobRef;
use crate::shared_kernel::shard_route::EpochMap;
use std::path::Path;

pub(crate) trait RestoreBlobWriter: Send + Sync {
    type Error;
    fn put(&self, bytes: &[u8], encoding: String) -> Result<ContentBlobRef, Self::Error>;
}

pub(crate) trait RestoreResources: Send + Sync {
    type Error;
    fn write_epochs(&self, root: &Path, epochs: &[EpochMap]) -> Result<(), Self::Error>;
    fn open_blob_writer(
        &self,
        root: &Path,
        externalize_bytes: usize,
    ) -> Result<Box<dyn RestoreBlobWriter<Error = Self::Error>>, Self::Error>;
}
