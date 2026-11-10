//! Count a complete bounded window at one fixed projection watermark.
use super::{
    archive_query_status::ArchiveQueryStatus,
    replay_cold_query::{replay_cold_query, ColdQueryScope},
};
use crate::app::{interfaces::http::api_error::ApiError, service_state::ServiceState};
use crate::projection::{LogQuery, PROJECTION_LOGGING_STORE};
use crate::query::domain::filter_evaluator::evaluate_filter;
use crate::query::domain::query_time::requires_archive_at;
use crate::shared_kernel::{
    event::SignalKind, query_expression::QueryExpressionV1, query_response::QueryResponseV1,
    query_v2::QueryRequestV2,
};
use chrono::{DateTime, Utc};
use std::time::Instant;

pub(crate) fn count_logs(
    state: &ServiceState,
    request: &QueryRequestV2,
    filter: Option<&QueryExpressionV1>,
) -> Result<QueryResponseV1, ApiError> {
    let started = Instant::now();
    let start = request
        .time_range
        .start
        .as_deref()
        .ok_or_else(|| ApiError::bad_request("invalid_query", "log count requires start"))?;
    let end = request
        .time_range
        .end
        .as_deref()
        .ok_or_else(|| ApiError::bad_request("invalid_query", "log count requires end"))?;
    let start_time = DateTime::parse_from_rfc3339(start)
        .map_err(|e| ApiError::bad_request("invalid_query", e.to_string()))?;
    let end_time = DateTime::parse_from_rfc3339(end)
        .map_err(|e| ApiError::bad_request("invalid_query", e.to_string()))?;
    if start_time >= end_time || end_time.signed_duration_since(start_time).num_seconds() > 86_400 {
        return Err(ApiError::bad_request(
            "invalid_query",
            "log count window must be positive and at most 24 hours",
        ));
    }
    let watermark = state.journal().last_cursor();
    state
        .projections()
        .catch_up(PROJECTION_LOGGING_STORE)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let replay = if requires_archive_at(start_time.with_timezone(&Utc), Utc::now()) {
        let replay = state
            .projections()
            .new_log_replay()
            .map_err(|e| ApiError::internal(e.to_string()))?;
        match replay_cold_query(
            state,
            &ColdQueryScope {
                project: &request.project,
                environment: request.environment.as_deref(),
                start: Some(start),
                end: Some(end),
            },
            SignalKind::Log,
            &replay,
        ) {
            ArchiveQueryStatus::Ready(_) => Some(replay),
            ArchiveQueryStatus::Unavailable(message) => {
                return Err(ApiError::temporarily_unavailable(message))
            }
            ArchiveQueryStatus::NotRequired => None,
        }
    } else {
        None
    };
    let mut query = LogQuery::for_project(&request.project);
    query.environment = request.environment.clone();
    query.start_time = Some(start.into());
    query.end_time = Some(end.into());
    query.limit = 1_000;
    query.max_cursor = Some(watermark);
    let (mut count, mut scanned, mut bytes) = (0u64, 0usize, 0usize);
    let mut source_budget = super::journal_log_page::ScanBudget::default();
    loop {
        let mut page = if let Some(replay) = &replay {
            replay.query(&query)
        } else {
            state.projections().query_logs(&query)
        }
        .map_err(|e| ApiError::bad_request("invalid_query", e.to_string()))?;
        if page.incomplete {
            page = super::journal_log_page::read_with_budget(
                state,
                &query,
                watermark,
                &mut source_budget,
            )
            .map_err(|e| ApiError::temporarily_unavailable(e.to_string()))?;
        }
        for record in &page.records {
            scanned += 1;
            let document =
                serde_json::to_value(record).map_err(|e| ApiError::internal(e.to_string()))?;
            bytes = bytes.saturating_add(
                serde_json::to_vec(&document)
                    .map_err(|e| ApiError::internal(e.to_string()))?
                    .len(),
            );
            if scanned > 250_000 || bytes > 64 * 1024 * 1024 {
                return Err(ApiError::bad_request(
                    "query_limit",
                    "exact log count exceeds 250000 rows or 64 MiB; narrow the window",
                ));
            }
            if filter
                .map(|filter| evaluate_filter(filter, &document))
                .transpose()
                .map_err(|e| ApiError::bad_request("invalid_query", e.to_string()))?
                .unwrap_or(true)
            {
                count += 1;
            }
        }
        if !page.has_more {
            break;
        }
        if page.next_cursor <= query.after_cursor {
            return Err(ApiError::internal("log count cursor did not advance"));
        }
        query.after_cursor = page.next_cursor;
    }
    Ok(QueryResponseV1::complete(
        serde_json::json!({"count":count,"exact":true}),
        None,
        watermark,
        scanned,
        1,
        started.elapsed(),
    ))
}
