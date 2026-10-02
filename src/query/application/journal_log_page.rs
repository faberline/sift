//! Read a bounded log page from the durable source after view eviction.
use crate::app::service_state::ServiceState;
use crate::projection::application::projection_runtime::ProjectionRuntime;
use crate::projection::{LogPage, LogQuery};
use anyhow::{bail, Context, Result};
use chrono::DateTime;

pub(crate) fn read(state: &ServiceState, query: &LogQuery, watermark: u64) -> Result<LogPage> {
    read_with_budget(state, query, watermark, &mut ScanBudget::default())
}
#[derive(Default)]
pub(crate) struct ScanBudget {
    scanned: usize,
    bytes: usize,
}
pub(crate) fn read_with_budget(
    state: &ServiceState,
    query: &LogQuery,
    watermark: u64,
    budget: &mut ScanBudget,
) -> Result<LogPage> {
    let start = query
        .start_time
        .as_deref()
        .map(DateTime::parse_from_rfc3339)
        .transpose()?;
    let end = query
        .end_time
        .as_deref()
        .map(DateTime::parse_from_rfc3339)
        .transpose()?;
    let mut after = query.after_cursor;
    let mut reader = state.journal_service().projection_read_session(after)?;
    let mut records = Vec::new();
    'scan: while after < watermark {
        let events = reader.read_next(64)?;
        if events.is_empty() {
            bail!("durable log source is incomplete below the query watermark");
        }
        for stored in events {
            if stored.cursor > watermark {
                after = watermark;
                break 'scan;
            }
            if stored.cursor <= after {
                bail!("journal query cursor did not advance");
            }
            budget.scanned += 1;
            budget.bytes = budget
                .bytes
                .checked_add(serde_json::to_vec(&stored)?.len())
                .context("journal query byte overflow")?;
            if budget.scanned > 250_000 || budget.bytes > 64 * 1024 * 1024 {
                bail!("query_limit: durable log scan exceeds 250000 rows or 64 MiB");
            }
            after = stored.cursor;
            if stored.event.signal != crate::SignalKind::Log
                || stored.event.project != query.project
                || query
                    .environment
                    .as_ref()
                    .is_some_and(|env| &stored.event.environment != env)
            {
                continue;
            }
            let occurred = DateTime::parse_from_rfc3339(&stored.event.occurred_at)?;
            if start.is_some_and(|start| occurred < start) || end.is_some_and(|end| occurred >= end)
            {
                continue;
            }
            records.push(ProjectionRuntime::log_record(&stored));
            if records.len() == query.limit {
                break;
            }
        }
        if records.len() == query.limit || after >= watermark {
            break;
        }
    }
    Ok(LogPage {
        records,
        next_cursor: after,
        projection_cursor: watermark,
        has_more: after < watermark,
        incomplete: false,
    })
}
