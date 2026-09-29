//! The Prometheus-compatible instant and range query endpoints.

use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Extension, Query, State};
use axum::Json;
use chrono::Utc;
use service_auth::RoleMapPrincipal;

use crate::access::interfaces::http::project_authorization::authorize_project_read;
use crate::query::application::prom_query::{prom_metric_query, prom_range_result};
use crate::query::domain::promql::{parse_promql, PromFunction};
use crate::query::domain::promql_evaluator::{
    ensure_complete_prom_metric_page, ensure_prom_range_work_budget, prom_aggregate,
    prom_latest_values, prom_nanos_to_seconds, prom_number, prom_rate_values, prom_series_labels,
    PROM_LOOKBACK_NANOS, PROM_MAX_RANGE_EVALUATIONS,
};
use crate::query::interfaces::http::prom_query_params::{
    nanos_rfc3339, parse_prom_duration_nanos, parse_prom_time_nanos, InstantQueryParams,
    RangeQueryParams,
};
use crate::{projection, ApiError, ServiceState};

pub(crate) async fn prometheus_instant_query(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Query(params): Query<InstantQueryParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    authorize_project_read(
        principal.as_ref().map(|principal| &principal.0),
        &params.project,
    )?;
    state
        .journal
        .ensure_queryable()
        .map_err(|error| ApiError::temporarily_unavailable(error.to_string()))?;
    let parsed = parse_promql(&params.query)
        .map_err(|error| ApiError::bad_request("bad_data", error.to_string()))?;
    let evaluation_time = params
        .time
        .as_deref()
        .map(parse_prom_time_nanos)
        .transpose()
        .map_err(|error| ApiError::bad_request("bad_data", error.to_string()))?
        .unwrap_or_else(|| Utc::now().timestamp_nanos_opt().unwrap_or(i64::MAX));
    state
        .projections
        .catch_up(projection::PROJECTION_METRIC_STORE)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let mut query = prom_metric_query(&params.project, params.environment, &parsed)?;
    query.start_time = evaluation_time
        .checked_sub(PROM_LOOKBACK_NANOS)
        .map(nanos_rfc3339)
        .transpose()
        .map_err(|error| ApiError::bad_request("bad_data", error.to_string()))?;
    query.end_time = evaluation_time
        .checked_add(1)
        .map(nanos_rfc3339)
        .transpose()
        .map_err(|error| ApiError::bad_request("bad_data", error.to_string()))?;
    let page = state
        .projections
        .query_metrics(&query)
        .map_err(|error| ApiError::bad_request("bad_data", error.to_string()))?;
    ensure_complete_prom_metric_page(&page)?;
    let mut latest = page
        .series
        .iter()
        .filter_map(|series| {
            prom_latest_values(&series.points, &[evaluation_time])
                .into_iter()
                .next()
                .flatten()
                .map(|value| (series, value))
        })
        .collect::<Vec<_>>();
    let result = match parsed.function {
        PromFunction::Raw => latest
            .drain(..)
            .map(|(series, value)| {
                serde_json::json!({
                    "metric": prom_series_labels(series),
                    "value": [prom_nanos_to_seconds(evaluation_time), prom_number(value)]
                })
            })
            .collect::<Vec<_>>(),
        PromFunction::Rate => page
            .series
            .iter()
            .filter_map(|series| {
                prom_rate_values(&series.points, &[evaluation_time])
                    .into_iter()
                    .next()
                    .flatten()
                    .map(|value| (series, value))
            })
            .map(|(series, value)| {
                serde_json::json!({
                    "metric": prom_series_labels(series),
                    "value": [prom_nanos_to_seconds(evaluation_time), prom_number(value)]
                })
            })
            .collect::<Vec<_>>(),
        function => {
            let values = latest
                .into_iter()
                .map(|(_, value)| value)
                .collect::<Vec<_>>();
            prom_aggregate(function, &values)
                .map(|value| {
                    vec![serde_json::json!({
                        "metric": {},
                        "value": [prom_nanos_to_seconds(evaluation_time), prom_number(value)]
                    })]
                })
                .unwrap_or_default()
        }
    };
    Ok(Json(serde_json::json!({
        "status": "success",
        "data": {"resultType": "vector", "result": result}
    })))
}

pub(crate) async fn prometheus_range_query(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Query(params): Query<RangeQueryParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    authorize_project_read(
        principal.as_ref().map(|principal| &principal.0),
        &params.project,
    )?;
    state
        .journal
        .ensure_queryable()
        .map_err(|error| ApiError::temporarily_unavailable(error.to_string()))?;
    let parsed = parse_promql(&params.query)
        .map_err(|error| ApiError::bad_request("bad_data", error.to_string()))?;
    let start = parse_prom_time_nanos(&params.start)
        .map_err(|error| ApiError::bad_request("bad_data", error.to_string()))?;
    let end = parse_prom_time_nanos(&params.end)
        .map_err(|error| ApiError::bad_request("bad_data", error.to_string()))?;
    let step = parse_prom_duration_nanos(&params.step)
        .map_err(|error| ApiError::bad_request("bad_data", error.to_string()))?;
    if start >= end || step <= 0 {
        return Err(ApiError::bad_request(
            "bad_data",
            "query_range requires start < end and step > 0",
        ));
    }
    let evaluation_count = (i128::from(end) - i128::from(start)) / i128::from(step) + 1;
    if evaluation_count > i128::from(PROM_MAX_RANGE_EVALUATIONS) {
        return Err(ApiError::bad_request(
            "bad_data",
            format!("query_range supports at most {PROM_MAX_RANGE_EVALUATIONS} evaluation steps"),
        ));
    }
    state
        .projections
        .catch_up(projection::PROJECTION_METRIC_STORE)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let mut query = prom_metric_query(&params.project, params.environment, &parsed)?;
    query.start_time = start
        .checked_sub(PROM_LOOKBACK_NANOS)
        .map(nanos_rfc3339)
        .transpose()
        .map_err(|error| ApiError::bad_request("bad_data", error.to_string()))?;
    query.end_time = end
        .checked_add(1)
        .map(nanos_rfc3339)
        .transpose()
        .map_err(|error| ApiError::bad_request("bad_data", error.to_string()))?;
    let page = state
        .projections
        .query_metrics(&query)
        .map_err(|error| ApiError::bad_request("bad_data", error.to_string()))?;
    ensure_complete_prom_metric_page(&page)?;
    let evaluation_count = usize::try_from(evaluation_count)
        .map_err(|_| ApiError::bad_request("bad_data", "invalid evaluation count"))?;
    ensure_prom_range_work_budget(page.series.len(), evaluation_count)?;
    let result = prom_range_result(&page.series, parsed.function, start, end, step);
    Ok(Json(serde_json::json!({
        "status": "success",
        "data": {"resultType": "matrix", "result": result}
    })))
}
