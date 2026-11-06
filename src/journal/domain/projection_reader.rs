//! A bounded journal read session used by projection adapters.
use crate::shared_kernel::stored_event::StoredEvent;
pub trait ProjectionReader: Send {
    type Error;
    fn read_next(&mut self, limit: usize) -> Result<Vec<StoredEvent>, Self::Error>;
}
