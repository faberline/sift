//! Executing a versioned query over logs, metrics or traces against the
//! projections, with the archive when the query reaches past them.

use std::time::Instant;

use anyhow::{Context, Result};

use crate::event::SignalKind;
use crate::query::application::archive_query_status::{
    apply_archive_query_status, ArchiveQueryStatus,
};
use crate::query::domain::filter_evaluator::evaluate_filter;
use crate::query::domain::query_cursor::{decode_query_cursor, encode_query_cursor};
use crate::query::domain::query_time::{cold_query_requested, parse_query_time_nanos};
use crate::query::interfaces::http::query_request_v1::{
    MetricFunctionV1, QueryRequestV1, QuerySignalV1,
};
use crate::query::interfaces::http::query_response_v1::QueryResponseV1;
use crate::{
    archive::application::replay_cold_query::replay_cold_query, projection, ApiError, ServiceState,
};

pub(in crate::query) fn execute_query_v1(
    state: &ServiceState,
    request: &QueryRequestV1,
) -> Result<QueryResponseV1, ApiError> {
    state
        .journal
        .ensure_queryable()
        .map_err(|error| ApiError::temporarily_unavailable(error.to_string()))?;
    let started = Instant::now();
    match &request.signal {
        QuerySignalV1::Logs { filter } => {
            state
                .projections
                .catch_up(projection::PROJECTION_LOGGING_STORE)
                .map_err(|error| ApiError::internal(error.to_string()))?;
            let watermark = state
                .projections
                .current_cursor(projection::PROJECTION_LOGGING_STORE)
                .map_err(|error| ApiError::internal(error.to_string()))?;
            let after_cursor = decode_query_cursor("logs", request.cursor.as_deref())?
                .map(|value| {
                    value.parse::<u64>().map_err(|_| {
                        ApiError::bad_request("invalid_cursor", "log cursor is not an integer")
                    })
                })
                .transpose()?
                .unwrap_or(0);
            let mut query = projection::LogQuery::for_project(&request.project);
            query.environment = request.environment.clone();
            query.start_time = request.time_range.start.clone();
            query.end_time = request.time_range.end.clone();
            query.after_cursor = after_cursor;
            query.limit = projection::MAX_LOG_QUERY_LIMIT;
            let (page, archive_status) = if cold_query_requested(request) {
                let archived = projection::LoggingProjection::new()
                    .map_err(|error| ApiError::internal(error.to_string()))?;
                let status = replay_cold_query(state, request, SignalKind::Log, &archived);
                let page = match status {
                    ArchiveQueryStatus::Ready(_) => archived.query(&query),
                    ArchiveQueryStatus::NotRequired | ArchiveQueryStatus::Unavailable(_) => {
                        state.projections.query_logs(&query)
                    }
                }
                .map_err(|error| ApiError::bad_request("invalid_query", error.to_string()))?;
                (page, status)
            } else {
                (
                    state.projections.query_logs(&query).map_err(|error| {
                        ApiError::bad_request("invalid_query", error.to_string())
                    })?,
                    ArchiveQueryStatus::NotRequired,
                )
            };
            let scanned = page.records.len();
            let scanned_cursor = page
                .records
                .last()
                .map(|record| record.cursor)
                .unwrap_or(after_cursor);
            let mut records = Vec::new();
            for record in page.records {
                if filter
                    .as_ref()
                    .map(|filter| {
                        serde_json::to_value(&record)
                            .map_err(anyhow::Error::from)
                            .and_then(|document| evaluate_filter(filter, &document))
                    })
                    .transpose()
                    .map_err(|error| ApiError::bad_request("invalid_query", error.to_string()))?
                    .unwrap_or(true)
                {
                    records.push(record);
                    if records.len() > request.limit {
                        break;
                    }
                }
            }
            let filtered_more = records.len() > request.limit;
            records.truncate(request.limit);
            let has_more = filtered_more || page.has_more;
            let next_cursor =
                has_more.then(|| encode_query_cursor("logs", &scanned_cursor.to_string()));
            let returned = records.len();
            Ok(apply_archive_query_status(
                QueryResponseV1::complete(
                    serde_json::json!({"records": records}),
                    next_cursor,
                    watermark,
                    scanned,
                    returned,
                    started.elapsed(),
                ),
                archive_status,
            ))
        }
        QuerySignalV1::Metrics {
            name,
            function,
            filter,
            ..
        } => {
            state
                .projections
                .catch_up(projection::PROJECTION_METRIC_STORE)
                .map_err(|error| ApiError::internal(error.to_string()))?;
            let watermark = state
                .projections
                .current_cursor(projection::PROJECTION_METRIC_STORE)
                .map_err(|error| ApiError::internal(error.to_string()))?;
            let mut query = projection::MetricQuery::for_project(&request.project);
            query.environment = request.environment.clone();
            query.name = name.clone();
            query.start_time = request.time_range.start.clone();
            query.end_time = request.time_range.end.clone();
            query.aggregation = match function {
                MetricFunctionV1::Raw => projection::MetricAggregation::Raw,
                MetricFunctionV1::Sum => projection::MetricAggregation::Sum,
                MetricFunctionV1::Avg => projection::MetricAggregation::Avg,
                MetricFunctionV1::Min => projection::MetricAggregation::Min,
                MetricFunctionV1::Max => projection::MetricAggregation::Max,
                MetricFunctionV1::Count => projection::MetricAggregation::Count,
                MetricFunctionV1::Rate => projection::MetricAggregation::Rate,
            };
            query.after_series_id = decode_query_cursor("metrics", request.cursor.as_deref())?;
            query.limit = projection::MAX_METRIC_QUERY_LIMIT;
            let (page, archive_status) = if cold_query_requested(request) {
                let archived = projection::MetricProjection::new();
                let status = replay_cold_query(state, request, SignalKind::Metric, &archived);
                let page = match status {
                    ArchiveQueryStatus::Ready(_) => archived.query(&query),
                    ArchiveQueryStatus::NotRequired | ArchiveQueryStatus::Unavailable(_) => {
                        state.projections.query_metrics(&query)
                    }
                }
                .map_err(|error| ApiError::bad_request("invalid_query", error.to_string()))?;
                (page, status)
            } else {
                (
                    state.projections.query_metrics(&query).map_err(|error| {
                        ApiError::bad_request("invalid_query", error.to_string())
                    })?,
                    ArchiveQueryStatus::NotRequired,
                )
            };
            let scanned = page.series.len();
            let scanned_cursor = page.series.last().map(|series| series.series_id.clone());
            let mut series = Vec::new();
            for item in page.series {
                if filter
                    .as_ref()
                    .map(|filter| {
                        serde_json::to_value(&item)
                            .map_err(anyhow::Error::from)
                            .and_then(|document| evaluate_filter(filter, &document))
                    })
                    .transpose()
                    .map_err(|error| ApiError::bad_request("invalid_query", error.to_string()))?
                    .unwrap_or(true)
                {
                    series.push(item);
                    if series.len() > request.limit {
                        break;
                    }
                }
            }
            let filtered_more = series.len() > request.limit;
            series.truncate(request.limit);
            let has_more = filtered_more || page.has_more;
            let next_cursor = has_more
                .then(|| {
                    scanned_cursor
                        .as_deref()
                        .map(|value| encode_query_cursor("metrics", value))
                })
                .flatten();
            let returned = series.len();
            Ok(apply_archive_query_status(
                QueryResponseV1::complete(
                    serde_json::json!({
                        "series": series,
                        "overflowed_series": page.overflowed_series,
                        "overflowed_points": page.overflowed_points
                    }),
                    next_cursor,
                    watermark,
                    scanned,
                    returned,
                    started.elapsed(),
                ),
                archive_status,
            ))
        }
        QuerySignalV1::Traces {
            service,
            operation,
            min_duration_ms,
            max_duration_ms,
            status,
            attributes,
            filter,
        } => {
            state
                .projections
                .catch_up(projection::PROJECTION_TRACE_STORE)
                .map_err(|error| ApiError::internal(error.to_string()))?;
            let watermark = state
                .projections
                .current_cursor(projection::PROJECTION_TRACE_STORE)
                .map_err(|error| ApiError::internal(error.to_string()))?;
            let mut query = projection::TraceQuery::for_project(&request.project);
            query.environment = request.environment.clone();
            query.start_time_unix_nano = request
                .time_range
                .start
                .as_deref()
                .map(parse_query_time_nanos)
                .transpose()?;
            query.end_time_unix_nano = request
                .time_range
                .end
                .as_deref()
                .map(parse_query_time_nanos)
                .transpose()?;
            query.service = service.clone();
            query.operation = operation.clone();
            query.min_duration_unix_nano =
                min_duration_ms.map(|value| value.saturating_mul(1_000_000));
            query.max_duration_unix_nano =
                max_duration_ms.map(|value| value.saturating_mul(1_000_000));
            query.status = status.clone();
            query.attributes = attributes
                .iter()
                .map(|(key, value)| {
                    serde_json::from_value(value.clone())
                        .map(|value| (key.clone(), value))
                        .with_context(|| {
                            format!("trace attribute `{key}` has an unsupported value")
                        })
                })
                .collect::<Result<_>>()
                .map_err(|error| ApiError::bad_request("invalid_query", error.to_string()))?;
            query.after_trace_id = decode_query_cursor("traces", request.cursor.as_deref())?;
            query.limit = projection::MAX_TRACE_QUERY_LIMIT;
            let (page, archive_status) = if cold_query_requested(request) {
                let archived = projection::TraceProjection::new();
                let status = replay_cold_query(state, request, SignalKind::Span, &archived);
                let page = match status {
                    ArchiveQueryStatus::Ready(_) => archived.query(&query),
                    ArchiveQueryStatus::NotRequired | ArchiveQueryStatus::Unavailable(_) => {
                        state.projections.query_traces(&query)
                    }
                }
                .map_err(|error| ApiError::bad_request("invalid_query", error.to_string()))?;
                (page, status)
            } else {
                (
                    state.projections.query_traces(&query).map_err(|error| {
                        ApiError::bad_request("invalid_query", error.to_string())
                    })?,
                    ArchiveQueryStatus::NotRequired,
                )
            };
            let scanned = page.traces.len();
            let scanned_cursor = page.traces.last().map(|trace| trace.trace_id.clone());
            let mut traces = Vec::new();
            for trace in page.traces {
                if filter
                    .as_ref()
                    .map(|filter| {
                        serde_json::to_value(&trace)
                            .map_err(anyhow::Error::from)
                            .and_then(|document| evaluate_filter(filter, &document))
                    })
                    .transpose()
                    .map_err(|error| ApiError::bad_request("invalid_query", error.to_string()))?
                    .unwrap_or(true)
                {
                    traces.push(trace);
                    if traces.len() > request.limit {
                        break;
                    }
                }
            }
            let filtered_more = traces.len() > request.limit;
            traces.truncate(request.limit);
            let has_more = filtered_more || page.has_more;
            let next_cursor = has_more
                .then(|| {
                    scanned_cursor
                        .as_deref()
                        .map(|value| encode_query_cursor("traces", value))
                })
                .flatten();
            let returned = traces.len();
            Ok(apply_archive_query_status(
                QueryResponseV1::complete(
                    serde_json::json!({"traces": traces}),
                    next_cursor,
                    watermark,
                    scanned,
                    returned,
                    started.elapsed(),
                ),
                archive_status,
            ))
        }
    }
}
