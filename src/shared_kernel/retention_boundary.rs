//! Sift keeps 180 days of events; older ones are rejected at ingest.

use chrono::{DateTime, Utc};

use crate::shared_kernel::event::OperationalEventV2 as EventEnvelope;

pub(crate) fn retention_rejection_at(
    event: &EventEnvelope,
    decision_time: DateTime<Utc>,
) -> Option<String> {
    let occurred = DateTime::parse_from_rfc3339(&event.occurred_at).ok()?;
    let cutoff = decision_time - chrono::Duration::days(180);
    (occurred.with_timezone(&Utc) < cutoff).then(|| {
        format!(
            "event `{}` occurred before Sift's 180-day retention boundary",
            event.event_id
        )
    })
}
