//! Each WAL port call uses the original SignalWal instance.

use crate::journal::domain::wal_store::{WalReader, WalStore};
use crate::journal::infrastructure::storage::wal::{SignalWal, SignalWalReader};
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::shared_kernel::stored_event::StoredEvent;
use anyhow::Result;

impl WalStore for SignalWal {
    type Error = anyhow::Error;
    fn append_batch(&self, events: &[StoredEvent]) -> Result<()> {
        SignalWal::append_batch(self, events)
    }
    fn compact_through(&self, watermarks: ArchiveWatermarks) -> Result<()> {
        SignalWal::compact_through(self, watermarks)
    }
    fn reader(&self, after: u64) -> Result<Box<dyn WalReader<Error = Self::Error>>> {
        Ok(Box::new(SignalWal::reader(self, after)?))
    }
}
impl WalReader for SignalWalReader {
    type Error = anyhow::Error;
    fn read_page(&mut self, item_limit: usize, byte_limit: usize) -> Result<Vec<StoredEvent>> {
        SignalWalReader::read_page(self, item_limit, byte_limit)
    }
}
