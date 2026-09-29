//! An event as the journal stores it: the cursor it was appended at, when Sift
//! accepted it, and the event itself.

use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;

use crate::shared_kernel::event::{IncomingEvent, OperationalEventV2 as EventEnvelope};

#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct StoredEvent {
    pub cursor: u64,
    /// Sift acceptance time. Exact event-id idempotency starts from this time.
    pub acknowledged_at: String,
    pub event: EventEnvelope,
}

#[derive(Deserialize)]
struct StoredEventWire {
    cursor: u64,
    acknowledged_at: String,
    event: IncomingEvent,
}

impl<'de> Deserialize<'de> for StoredEvent {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = StoredEventWire::deserialize(deserializer)?;
        Ok(Self {
            cursor: wire.cursor,
            acknowledged_at: wire.acknowledged_at,
            event: wire.event.into_inner(),
        })
    }
}
