//! The pure projection contract. Adapters preserve their native errors.

use crate::projection::domain::runtime_metadata::ProjectionDescriptor;
use crate::shared_kernel::sha256_hex::sha256_hex;
use crate::shared_kernel::stored_event::StoredEvent;

pub trait Projection: Send + Sync + 'static {
    type Error;

    fn descriptor(&self) -> ProjectionDescriptor;
    fn apply_idempotent(&self, event: &StoredEvent) -> Result<(), Self::Error>;
    fn snapshot(&self) -> Result<Vec<u8>, Self::Error>;
    fn restore(&self, state: &[u8]) -> Result<(), Self::Error>;

    fn checkpoint_committed(&self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn semantic_digest(&self) -> Result<String, Self::Error> {
        Ok(sha256_hex(self.snapshot()?))
    }
}
