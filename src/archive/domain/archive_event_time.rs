//! Reading an archived event's occurrence and acknowledgement times, and a
//! query's time bounds.

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::shared_kernel::stored_event::StoredEvent;

#[derive(Debug, Error)]
pub(in crate::archive) enum ArchiveTimeError {
    #[error("archive query {name} must be RFC3339")]
    QueryTime {
        name: &'static str,
        #[source]
        source: chrono::ParseError,
    },
    #[error("archive event occurred_at must be RFC3339")]
    OccurredAtNotRfc3339(#[source] chrono::ParseError),
    #[error("archive event occurred_at is outside the nanosecond range")]
    OccurredAtOutOfRange,
    #[error("archive event acknowledged_at must be RFC3339")]
    AcknowledgedAtNotRfc3339(#[source] chrono::ParseError),
    #[error("archive event acknowledged_at is outside the nanosecond range")]
    AcknowledgedAtOutOfRange,
    #[error("archive segment must contain at least one event")]
    EmptySegment,
}

pub(in crate::archive) fn parse_optional_archive_time(
    name: &'static str,
    value: Option<&str>,
) -> Result<Option<DateTime<Utc>>, ArchiveTimeError> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map_err(|source| ArchiveTimeError::QueryTime { name, source })
                .map(|value| value.with_timezone(&Utc))
        })
        .transpose()
}

pub(in crate::archive) fn event_time_unix_nano(
    event: &StoredEvent,
) -> Result<i64, ArchiveTimeError> {
    DateTime::parse_from_rfc3339(&event.event.occurred_at)
        .map_err(ArchiveTimeError::OccurredAtNotRfc3339)?
        .timestamp_nanos_opt()
        .ok_or(ArchiveTimeError::OccurredAtOutOfRange)
}

pub(in crate::archive) fn acknowledgement_time_unix_nano(
    event: &StoredEvent,
) -> Result<i64, ArchiveTimeError> {
    DateTime::parse_from_rfc3339(&event.acknowledged_at)
        .map_err(ArchiveTimeError::AcknowledgedAtNotRfc3339)?
        .timestamp_nanos_opt()
        .ok_or(ArchiveTimeError::AcknowledgedAtOutOfRange)
}

pub(in crate::archive) fn acknowledgement_time_bounds(
    events: &[StoredEvent],
) -> Result<(i64, i64), ArchiveTimeError> {
    let mut values = events.iter().map(acknowledgement_time_unix_nano);
    let first = values
        .next()
        .transpose()?
        .ok_or(ArchiveTimeError::EmptySegment)?;
    values.try_fold((first, first), |(minimum, maximum), value| {
        let value = value?;
        Ok::<_, ArchiveTimeError>((minimum.min(value), maximum.max(value)))
    })
}
