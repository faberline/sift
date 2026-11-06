//! SegmentCodecPort isolates archive I/O.

use crate::shared_kernel::archive_manifest::ArchiveSegment;
use crate::shared_kernel::stored_event::StoredEvent;

pub(crate) trait SegmentCodecPort: Send + Sync {
    type PartitionError;
    fn partition(&self, event: &StoredEvent) -> Result<String, Self::PartitionError>;
    type Error;
    fn encode_parquet(&self, events: &[StoredEvent]) -> Result<Vec<u8>, Self::Error>;
    fn decode_parquet(&self, bytes: &[u8]) -> Result<Vec<StoredEvent>, Self::Error>;
    fn verify_archive_segment(
        &self,
        segment: &ArchiveSegment,
        events: &[StoredEvent],
    ) -> Result<(), Self::Error>;
}
