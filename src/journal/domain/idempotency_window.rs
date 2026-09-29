//! The idempotency window: six hours of hourly acknowledgement generations, and
//! whether an event is still inside it.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::shared_kernel::stored_event::StoredEvent;

pub(in crate::journal) const GENERATION_SECONDS: i64 = 60 * 60;

pub const IDEMPOTENCY_WINDOW_SECONDS: i64 = 6 * 60 * 60;

pub(in crate::journal) fn generation_for(now: DateTime<Utc>) -> i64 {
    now.timestamp().div_euclid(GENERATION_SECONDS)
}

pub(in crate::journal) fn oldest_generation(now: DateTime<Utc>) -> i64 {
    now.timestamp()
        .saturating_sub(IDEMPOTENCY_WINDOW_SECONDS)
        .div_euclid(GENERATION_SECONDS)
}

pub(in crate::journal) fn acknowledged_generation(event: &StoredEvent) -> Result<i64> {
    Ok(DateTime::parse_from_rfc3339(&event.acknowledged_at)
        .context("acknowledged_at must be RFC3339")?
        .timestamp()
        .div_euclid(GENERATION_SECONDS))
}

pub(in crate::journal) fn acknowledged_nanos(event: &StoredEvent) -> Result<i64> {
    DateTime::parse_from_rfc3339(&event.acknowledged_at)
        .context("acknowledged_at must be RFC3339")?
        .timestamp_nanos_opt()
        .context("acknowledged_at is outside the nanosecond range")
}

pub(in crate::journal) fn event_is_active_at(
    event: &StoredEvent,
    now: DateTime<Utc>,
) -> Result<bool> {
    let cutoff = (now - chrono::Duration::seconds(IDEMPOTENCY_WINDOW_SECONDS))
        .timestamp_nanos_opt()
        .context("dedupe activity cutoff is outside the nanosecond range")?;
    Ok(acknowledged_nanos(event)? >= cutoff)
}
