//! Canonical WAL append, compaction and bounded reads.

use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::shared_kernel::stored_event::StoredEvent;

pub(crate) trait WalReader: Send {
    type Error;
    fn read_page(
        &mut self,
        item_limit: usize,
        byte_limit: usize,
    ) -> Result<Vec<StoredEvent>, Self::Error>;
}
pub(crate) trait WalStore: Send + Sync {
    type Error;
    fn append_batch(&self, events: &[StoredEvent]) -> Result<(), Self::Error>;
    fn compact_through(&self, watermarks: ArchiveWatermarks) -> Result<(), Self::Error>;
    fn reader(&self, after: u64) -> Result<Box<dyn WalReader<Error = Self::Error>>, Self::Error>;
}

impl<T: WalStore + ?Sized> WalStore for std::sync::Arc<T> {
    type Error = T::Error;
    fn append_batch(&self, events: &[StoredEvent]) -> Result<(), Self::Error> {
        self.as_ref().append_batch(events)
    }
    fn compact_through(&self, watermarks: ArchiveWatermarks) -> Result<(), Self::Error> {
        self.as_ref().compact_through(watermarks)
    }
    fn reader(&self, after: u64) -> Result<Box<dyn WalReader<Error = Self::Error>>, Self::Error> {
        self.as_ref().reader(after)
    }
}
