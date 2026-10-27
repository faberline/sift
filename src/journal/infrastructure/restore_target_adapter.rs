//! Preserve the original epoch writer and separate restore blob instance.

use crate::journal::domain::restore_target::{RestoreBlobWriter, RestoreResources};
use crate::journal::infrastructure::storage::blob_store::BlobStore;
use crate::journal::infrastructure::storage::shard_router::write_epoch_maps;
use crate::shared_kernel::event::ContentBlobRef;
use crate::shared_kernel::shard_route::EpochMap;
use anyhow::Result;
use std::path::Path;

pub(crate) struct NativeRestoreResources;
struct NativeBlobWriter(BlobStore);

impl RestoreBlobWriter for NativeBlobWriter {
    type Error = anyhow::Error;
    fn put(&self, bytes: &[u8], encoding: String) -> Result<ContentBlobRef> {
        self.0.put(bytes, encoding)
    }
}

impl RestoreResources for NativeRestoreResources {
    type Error = anyhow::Error;
    fn write_epochs(&self, root: &Path, epochs: &[EpochMap]) -> Result<()> {
        write_epoch_maps(root, epochs)
    }

    fn open_blob_writer(
        &self,
        root: &Path,
        externalize_bytes: usize,
    ) -> Result<Box<dyn RestoreBlobWriter<Error = Self::Error>>> {
        Ok(Box::new(NativeBlobWriter(BlobStore::open(
            root,
            externalize_bytes,
        )?)))
    }
}
