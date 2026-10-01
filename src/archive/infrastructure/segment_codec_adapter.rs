//! Use the existing archive I/O operations and error chains.

use crate::archive::domain::segment_codec_port::SegmentCodecPort;
use crate::shared_kernel::archive_manifest::ArchiveSegment;
use crate::shared_kernel::stored_event::StoredEvent;
use anyhow::Result;

pub(crate) struct NativeSegmentCodecPort;

impl SegmentCodecPort for NativeSegmentCodecPort {
    type PartitionError = storage_segment::SegmentError;
    fn partition(&self, event: &StoredEvent) -> Result<String, Self::PartitionError> {
        storage_segment::Partitioner::partition(
            &crate::archive::infrastructure::parquet_event_codec::SiftSignalPartitioner,
            event,
        )
    }

    type Error = anyhow::Error;
    fn encode_parquet(&self, events: &[StoredEvent]) -> Result<Vec<u8>> {
        crate::archive::infrastructure::parquet_event_codec::encode_parquet(events)
    }
    fn decode_parquet(&self, bytes: &[u8]) -> Result<Vec<StoredEvent>> {
        crate::archive::infrastructure::parquet_event_codec::decode_parquet(bytes)
    }
    fn verify_archive_segment(
        &self,
        segment: &ArchiveSegment,
        events: &[StoredEvent],
    ) -> Result<()> {
        crate::archive::infrastructure::archive_integrity::verify_archive_segment(segment, events)
    }
}
