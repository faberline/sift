//! OpenAPI descriptions for stored_event data. Runtime bytes use the original models.

use crate::EventEnvelope;

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct StoredEvent {
    pub cursor: u64,
    /// Sift acceptance time. Exact event-id idempotency starts from this time.
    pub acknowledged_at: String,
    pub event: EventEnvelope,
}

schema_adapter!(crate::shared_kernel::stored_event::StoredEvent, StoredEvent);
