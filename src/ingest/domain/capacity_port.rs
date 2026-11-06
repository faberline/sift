//! Local capacity preflight without a concrete disk guard.

pub(crate) trait IngestCapacity: Send + Sync {
    type Error;
    fn preflight(&self, bytes: u64) -> Result<(), Self::Error>;
}
