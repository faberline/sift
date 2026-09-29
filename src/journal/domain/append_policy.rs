//! The rules a replicated append follows: one signal per non-empty batch, the
//! decision time it is acknowledged at, and how retention fences merge.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::journal::domain::retention_fence::RetentionFenceV1;
use crate::EventEnvelope;

pub(in crate::journal) fn validate_events(events: &[EventEnvelope]) -> Result<()> {
    if events.is_empty() {
        bail!("Sift Raft batch must not be empty");
    }
    let signal = events[0].signal;
    if events.iter().any(|event| event.signal != signal) {
        bail!("Sift Raft batch must contain exactly one signal");
    }
    for event in events {
        event.validate()?;
    }
    Ok(())
}

pub(in crate::journal) fn append_decision_time(
    acknowledged_at: Option<&str>,
    events: &[EventEnvelope],
) -> Result<DateTime<Utc>> {
    if let Some(acknowledged_at) = acknowledged_at {
        return Ok(DateTime::parse_from_rfc3339(acknowledged_at)
            .context("Sift Raft acknowledgement time must be RFC3339")?
            .with_timezone(&Utc));
    }

    // Deterministic read compatibility for pre-v6 commands. New candidates
    // always send the explicit field. Never consult a voter's local clock in
    // state-machine apply.
    events
        .iter()
        .try_fold(None::<DateTime<Utc>>, |latest, event| {
            let observed = DateTime::parse_from_rfc3339(&event.observed_at)
                .context("legacy Sift command observed_at must be RFC3339")?
                .with_timezone(&Utc);
            anyhow::Ok(Some(latest.map_or(observed, |latest| latest.max(observed))))
        })?
        .context("legacy Sift append command must contain an event")
}

pub(in crate::journal) fn validate_retention_fence(fence: &RetentionFenceV1) -> Result<()> {
    if fence.target_generation == 0
        || !fence.source_manifest_uri.starts_with("gs://")
        || fence.source_manifest_sha256.len() != 64
        || !fence
            .source_manifest_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || chrono::DateTime::parse_from_rfc3339(&fence.evaluate_at).is_err()
    {
        bail!("Sift retention fence metadata is invalid");
    }
    Ok(())
}

pub(in crate::journal) fn merge_pending_retention(
    current: Option<RetentionFenceV1>,
    incoming: Option<RetentionFenceV1>,
) -> Result<Option<RetentionFenceV1>> {
    match (current, incoming) {
        (None, incoming) => Ok(incoming),
        (current, None) => Ok(current),
        (Some(current), Some(incoming))
            if current.target_generation == incoming.target_generation =>
        {
            if current != incoming {
                bail!(
                    "Sift retention fences disagree for generation {}",
                    current.target_generation
                );
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
