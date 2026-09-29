//! Reading an archived event's occurrence and acknowledgement times, and a
//! query's time bounds.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::shared_kernel::stored_event::StoredEvent;

pub(in crate::archive) fn parse_optional_archive_time(
    name: &str,
    value: Option<&str>,
) -> Result<Option<DateTime<Utc>>> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .with_context(|| format!("archive query {name} must be RFC3339"))
                .map(|value| value.with_timezone(&Utc))
        })
        .transpose()
}

pub(in crate::archive) fn event_time_unix_nano(event: &StoredEvent) -> Result<i64> {
    DateTime::parse_from_rfc3339(&event.event.occurred_at)
        .context("archive event occurred_at must be RFC3339")?
        .timestamp_nanos_opt()
        .context("archive event occurred_at is outside the nanosecond range")
}

pub(in crate::archive) fn acknowledgement_time_unix_nano(event: &StoredEvent) -> Result<i64> {
    DateTime::parse_from_rfc3339(&event.acknowledged_at)
        .context("archive event acknowledged_at must be RFC3339")?
        .timestamp_nanos_opt()
        .context("archive event acknowledged_at is outside the nanosecond range")
}

pub(in crate::archive) fn acknowledgement_time_bounds(
    events: &[StoredEvent],
) -> Result<(i64, i64)> {
    let mut values = events.iter().map(acknowledgement_time_unix_nano);
    let first = values
        .next()
        .transpose()?
        .context("archive segment must contain at least one event")?;
    values.try_fold((first, first), |(minimum, maximum), value| {
        let value = value?;
        anyhow::Ok((minimum.min(value), maximum.max(value)))
    })
}
