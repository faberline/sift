//! The idempotency window: six hours of hourly acknowledgement generations, and
//! whether an event is still inside it.

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::shared_kernel::stored_event::StoredEvent;

pub(crate) const GENERATION_SECONDS: i64 = 60 * 60;

pub const IDEMPOTENCY_WINDOW_SECONDS: i64 = 6 * 60 * 60;

pub(crate) fn generation_for(now: DateTime<Utc>) -> i64 {
    now.timestamp().div_euclid(GENERATION_SECONDS)
}

pub(crate) fn oldest_generation(now: DateTime<Utc>) -> i64 {
    now.timestamp()
        .saturating_sub(IDEMPOTENCY_WINDOW_SECONDS)
        .div_euclid(GENERATION_SECONDS)
}

#[derive(Debug, Error)]
pub(crate) enum IdempotencyWindowError {
    #[error("acknowledged_at must be RFC3339")]
    AcknowledgedAtNotRfc3339(#[source] chrono::ParseError),
    #[error("acknowledged_at is outside the nanosecond range")]
    AcknowledgedAtOutOfRange,
    #[error("dedupe activity cutoff is outside the nanosecond range")]
    CutoffOutOfRange,
}

pub(crate) fn acknowledged_generation(event: &StoredEvent) -> Result<i64, IdempotencyWindowError> {
    Ok(DateTime::parse_from_rfc3339(&event.acknowledged_at)
        .map_err(IdempotencyWindowError::AcknowledgedAtNotRfc3339)?
        .timestamp()
        .div_euclid(GENERATION_SECONDS))
}

pub(crate) fn acknowledged_nanos(event: &StoredEvent) -> Result<i64, IdempotencyWindowError> {
    DateTime::parse_from_rfc3339(&event.acknowledged_at)
        .map_err(IdempotencyWindowError::AcknowledgedAtNotRfc3339)?
        .timestamp_nanos_opt()
        .ok_or(IdempotencyWindowError::AcknowledgedAtOutOfRange)
}

pub(crate) fn event_is_active_at(
    event: &StoredEvent,
    now: DateTime<Utc>,
) -> Result<bool, IdempotencyWindowError> {
    let cutoff = (now - chrono::Duration::seconds(IDEMPOTENCY_WINDOW_SECONDS))
        .timestamp_nanos_opt()
        .ok_or(IdempotencyWindowError::CutoffOutOfRange)?;
    Ok(acknowledged_nanos(event)? >= cutoff)
}
