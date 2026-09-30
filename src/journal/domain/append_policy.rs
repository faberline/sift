//! The rules a replicated append follows: one signal per non-empty batch, the
//! decision time it is acknowledged at, and how retention fences merge.

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::journal::domain::retention_fence::RetentionFenceV1;
use crate::{EventEnvelope, EventError};

#[derive(Debug, Error)]
pub(in crate::journal) enum AppendPolicyError {
    #[error("Sift Raft batch must not be empty")]
    EmptyBatch,
    #[error("Sift Raft batch must contain exactly one signal")]
    MixedSignals,
    #[error(transparent)]
    Event(#[from] EventError),
    #[error("Sift Raft acknowledgement time must be RFC3339")]
    AcknowledgementTime(#[source] chrono::ParseError),
    #[error("legacy Sift command observed_at must be RFC3339")]
    LegacyObservedAt(#[source] chrono::ParseError),
    #[error("legacy Sift append command must contain an event")]
    LegacyWithoutEvent,
    #[error("Sift retention fence metadata is invalid")]
    InvalidRetentionFence,
    #[error("Sift retention fences disagree for generation {0}")]
    RetentionFenceConflict(u64),
}

pub(in crate::journal) fn validate_events(
    events: &[EventEnvelope],
) -> Result<(), AppendPolicyError> {
    if events.is_empty() {
        return Err(AppendPolicyError::EmptyBatch);
    }
    let signal = events[0].signal;
    if events.iter().any(|event| event.signal != signal) {
        return Err(AppendPolicyError::MixedSignals);
    }
    for event in events {
        event.validate()?;
    }
    Ok(())
}

pub(in crate::journal) fn append_decision_time(
    acknowledged_at: Option<&str>,
    events: &[EventEnvelope],
) -> Result<DateTime<Utc>, AppendPolicyError> {
    if let Some(acknowledged_at) = acknowledged_at {
        return Ok(DateTime::parse_from_rfc3339(acknowledged_at)
            .map_err(AppendPolicyError::AcknowledgementTime)?
            .with_timezone(&Utc));
    }

    // Deterministic read compatibility for pre-v6 commands. New candidates
    // always send the explicit field. Never consult a voter's local clock in
    // state-machine apply.
    events
        .iter()
        .try_fold(None::<DateTime<Utc>>, |latest, event| {
            let observed = DateTime::parse_from_rfc3339(&event.observed_at)
                .map_err(AppendPolicyError::LegacyObservedAt)?
                .with_timezone(&Utc);
            Ok::<_, AppendPolicyError>(Some(latest.map_or(observed, |latest| latest.max(observed))))
        })?
        .ok_or(AppendPolicyError::LegacyWithoutEvent)
}

pub(in crate::journal) fn validate_retention_fence(
    fence: &RetentionFenceV1,
) -> Result<(), AppendPolicyError> {
    if fence.target_generation == 0
        || !fence.source_manifest_uri.starts_with("gs://")
        || fence.source_manifest_sha256.len() != 64
        || !fence
            .source_manifest_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || chrono::DateTime::parse_from_rfc3339(&fence.evaluate_at).is_err()
    {
        return Err(AppendPolicyError::InvalidRetentionFence);
    }
    Ok(())
}

pub(in crate::journal) fn merge_pending_retention(
    current: Option<RetentionFenceV1>,
    incoming: Option<RetentionFenceV1>,
) -> Result<Option<RetentionFenceV1>, AppendPolicyError> {
    match (current, incoming) {
        (None, incoming) => Ok(incoming),
        (current, None) => Ok(current),
        (Some(current), Some(incoming))
            if current.target_generation == incoming.target_generation =>
        {
            if current != incoming {
                return Err(AppendPolicyError::RetentionFenceConflict(
                    current.target_generation,
                ));
            }
            Ok(Some(current))
        }
        (Some(current), Some(incoming)) => Ok(Some(
            if current.target_generation > incoming.target_generation {
                current
            } else {
                incoming
            },
        )),
    }
}
