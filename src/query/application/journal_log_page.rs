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
    let mut page_scanned = 0usize;
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
            page_scanned += 1;
            budget.bytes = budget
                .bytes
                .checked_add(serde_json::to_vec(&stored)?.len())
                .context("journal query byte overflow")?;
            if budget.scanned > 250_000 || budget.bytes > 64 * 1024 * 1024 {
                bail!("query_limit: durable log scan exceeds 250000 rows or 64 MiB");
            }
            after = stored.cursor;
            if stored.event.signal == crate::SignalKind::Log
                && stored.event.project == query.project
                && !query
                    .environment
                    .as_ref()
                    .is_some_and(|env| &stored.event.environment != env)
            {
                let occurred = DateTime::parse_from_rfc3339(&stored.event.occurred_at)?;
                if !start.is_some_and(|start| occurred < start)
                    && !end.is_some_and(|end| occurred >= end)
                {
                    records.push(ProjectionRuntime::log_record(&stored));
                }
            }
            if page_scanned == query.limit {
                break 'scan;
            }
        }
        if after >= watermark {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_kernel::event::{EventEnvelope, SignalKind};
    use chrono::{Duration, Utc};

    fn source() -> (tempfile::TempDir, ServiceState, LogQuery) {
        let root = tempfile::tempdir().unwrap();
        let state = ServiceState::open(root.path()).unwrap();
        let now = Utc::now();
        for (id, seconds) in [("future", 10), ("match", 0), ("tail", 11)] {
            let mut event = EventEnvelope::for_project(
                "paging-test",
                "test",
                id,
                SignalKind::Log,
                serde_json::json!({"message":id}),
            );
            event
                .resource
                .insert("service.name".into(), "sparse-test".into());
            event.occurred_at = (now + Duration::seconds(seconds)).to_rfc3339();
            state.journal().append(event).unwrap();
        }
        let mut query = LogQuery::for_project("paging-test");
        query.start_time = Some(now.to_rfc3339());
        query.end_time = Some((now + Duration::seconds(1)).to_rfc3339());
        query.limit = 1;
        (root, state, query)
    }

    #[test]
    fn sparse_pages_advance_over_empty_and_out_of_order_time_windows() {
        let (_root, state, mut query) = source();
        let first = read(&state, &query, 3).unwrap();
        assert!(first.records.is_empty());
        assert_eq!(first.next_cursor, 1);
        assert!(first.has_more);
        query.after_cursor = first.next_cursor;
        let second = read(&state, &query, 3).unwrap();
        assert_eq!(second.records[0].event_id, "match");
        assert_eq!(second.next_cursor, 2);
        assert!(second.has_more);
        query.after_cursor = second.next_cursor;
        let last = read(&state, &query, 3).unwrap();
        assert!(last.records.is_empty());
        assert_eq!(last.next_cursor, 3);
        assert!(!last.has_more);
    }

    #[test]
    fn sparse_pages_keep_the_cumulative_scan_and_byte_limits() {
        let (_root, state, query) = source();
        for mut budget in [
            ScanBudget {
                scanned: 250_000,
                bytes: 0,
            },
            ScanBudget {
                scanned: 0,
                bytes: 64 * 1024 * 1024,
            },
        ] {
            let error = read_with_budget(&state, &query, 3, &mut budget).unwrap_err();
            assert!(error.to_string().contains("query_limit"));
        }
    }
}
