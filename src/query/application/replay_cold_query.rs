//! Answering a cold query from the archive, joined to the local hot events
//! after it.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::app::service_state::ServiceState;
use crate::query::application::archive_query_status::ArchiveQueryStatus;
use crate::shared_kernel::event::SignalKind;
use crate::shared_kernel::event_query::EventQuery;

/// The part of a query a cold-query replay reads: the project, the optional
/// environment, and the RFC 3339 time range.
pub(crate) struct ColdQueryScope<'a> {
    pub(crate) project: &'a str,
    pub(crate) environment: Option<&'a str>,
    pub(crate) start: Option<&'a str>,
    pub(crate) end: Option<&'a str>,
}

pub(crate) fn replay_cold_query<Q, P>(
    state: &ServiceState,
    scope: &ColdQueryScope<'_>,
    signal: SignalKind,
    projection: &crate::projection::application::replay_service::ReplayService<Q, P>,
) -> ArchiveQueryStatus {
    let replay = crate::archive::application::replay_committed_events::replay_committed_events(
        state.journal().storage().root(),
        signal,
        scope.project,
        scope.environment,
        scope.start,
        scope.end,
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
        replay_local_events_after_archive(state, scope, signal, replay.watermark, projection)
    {
        return ArchiveQueryStatus::Unavailable(format!(
            "local hot data could not be joined to the archive: {error}"
        ));
    }
    ArchiveQueryStatus::Ready(replay)
}

fn replay_local_events_after_archive<Q, P>(
    state: &ServiceState,
    scope: &ColdQueryScope<'_>,
    signal: SignalKind,
    mut after: u64,
    projection: &crate::projection::application::replay_service::ReplayService<Q, P>,
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
            if event.event.project != scope.project
                || scope
                    .environment
                    .is_some_and(|environment| event.event.environment != environment)
            {
                continue;
            }
            let occurred = DateTime::parse_from_rfc3339(&event.event.occurred_at)
                .context("local event occurred_at must be RFC3339")?
                .with_timezone(&Utc);
            let start = scope
                .start
                .map(DateTime::parse_from_rfc3339)
                .transpose()?
                .map(|value| value.with_timezone(&Utc));
            let end = scope
                .end
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
