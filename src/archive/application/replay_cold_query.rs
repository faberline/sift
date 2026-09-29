//! Answering a cold query from the archive, joined to the local hot events
//! after it.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::event::SignalKind;
use crate::journal::domain::event_query::EventQuery;
use crate::query::application::archive_query_status::ArchiveQueryStatus;
use crate::query::interfaces::http::query_request_v1::QueryRequestV1;
use crate::ServiceState;

pub(crate) fn replay_cold_query(
    state: &ServiceState,
    request: &QueryRequestV1,
    signal: SignalKind,
    projection: &dyn crate::projection::Projection,
) -> ArchiveQueryStatus {
    let replay = crate::archive::application::replay_committed_events::replay_committed_events(
        state.journal().storage().root(),
        signal,
        &request.project,
        request.environment.as_deref(),
        request.time_range.start.as_deref(),
        request.time_range.end.as_deref(),
        |event| projection.apply_idempotent(&event),
    );
    let replay = match replay {
        Ok(Some(replay)) => replay,
        Ok(None) => {
            return ArchiveQueryStatus::Unavailable(
                "archive manifest is not committed for the requested cold time range".to_string(),
            )
        }
        Err(error) => {
            return ArchiveQueryStatus::Unavailable(format!(
                "archive is unavailable for the requested cold time range: {error}"
            ))
        }
    };
    if let Err(error) =
        replay_local_events_after_archive(state, request, signal, replay.watermark, projection)
    {
        return ArchiveQueryStatus::Unavailable(format!(
            "local hot data could not be joined to the archive: {error}"
        ));
    }
    ArchiveQueryStatus::Ready(replay)
}

fn replay_local_events_after_archive(
    state: &ServiceState,
    request: &QueryRequestV1,
    signal: SignalKind,
    mut after: u64,
    projection: &dyn crate::projection::Projection,
) -> Result<()> {
    loop {
        let events = state.journal().query(EventQuery {
            signal: Some(signal),
            after,
            limit: 10_000,
        })?;
        let Some(last) = events.last() else {
            break;
        };
        after = last.cursor;
        for event in events {
            if event.event.project != request.project
                || request
                    .environment
                    .as_ref()
                    .is_some_and(|environment| event.event.environment != *environment)
            {
                continue;
            }
            let occurred = DateTime::parse_from_rfc3339(&event.event.occurred_at)
                .context("local event occurred_at must be RFC3339")?
                .with_timezone(&Utc);
            let start = request
                .time_range
                .start
                .as_deref()
                .map(DateTime::parse_from_rfc3339)
                .transpose()?
                .map(|value| value.with_timezone(&Utc));
            let end = request
                .time_range
                .end
                .as_deref()
                .map(DateTime::parse_from_rfc3339)
                .transpose()?
                .map(|value| value.with_timezone(&Utc));
            if start.is_some_and(|start| occurred < start) || end.is_some_and(|end| occurred >= end)
            {
                continue;
            }
            projection.apply_idempotent(&event)?;
        }
    }
    Ok(())
}
