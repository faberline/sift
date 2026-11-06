//! Decode one source record into the shared operational event.

use crate::collector::domain::record::RawRecord;
use crate::shared_kernel::event::OperationalEventV2;

pub(crate) trait RecordDecoder {
    type Error;
    fn decode(&self, record: RawRecord) -> Result<OperationalEventV2, Self::Error>;
}
