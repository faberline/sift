//! Use the same capacity guard held by journal admission and lifecycle.

use crate::ingest::domain::capacity_port::IngestCapacity;
use std::sync::Arc;

pub(crate) struct NativeIngestCapacity {
    pub(crate) capacity: Arc<storage_durable::CapacityGuard>,
}

impl IngestCapacity for NativeIngestCapacity {
    type Error = storage_durable::CapacityError;
    fn preflight(&self, bytes: u64) -> Result<(), Self::Error> {
        self.capacity.preflight(bytes)
    }
}
