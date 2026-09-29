//! Correlating: the logs, metrics and traces that share a trace, span,
//! service or attributes.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::rejection::JsonRejection;
use axum::extract::{Extension, State};
use axum::Json;
use service_auth::RoleMapPrincipal;

use crate::access::interfaces::http::project_authorization::authorize_project_read;
use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::projection;
use crate::query::domain::query_time::parse_query_time_nanos;
use crate::query::interfaces::http::phase_one::{CorrelationRequestV1, CorrelationResponseV1};

pub(crate) async fn correlate_v1(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    payload: Result<Json<CorrelationRequestV1>, JsonRejection>,
) -> Result<Json<CorrelationResponseV1>, ApiError> {
    let Json(request) =
        payload.map_err(|error| ApiError::bad_request("invalid_json", error.body_text()))?;
    request
        .validate()
        .map_err(|error| ApiError::bad_request("invalid_correlation", error.to_string()))?;
    authorize_project_read(
        principal.as_ref().map(|principal| &principal.0),
        &request.project,
    )?;
    state
        .journal
        .ensure_queryable()
        .map_err(|error| ApiError::temporarily_unavailable(error.to_string()))?;
    for projection in [
        projection::PROJECTION_LOGGING_STORE,
        projection::PROJECTION_METRIC_STORE,
        projection::PROJECTION_TRACE_STORE,
    ] {
        state
            .projections
            .catch_up(projection)
            .map_err(|error| ApiError::internal(error.to_string()))?;
    }
    let attributes = request
        .attributes
        .iter()
        .map(|(key, value)| {
            serde_json::from_value(value.clone())
                .map(|value| (key.clone(), value))
                .with_context(|| format!("attribute `{key}` has an unsupported value"))
        })
        .collect::<Result<BTreeMap<_, _>>>()
        .map_err(|error| ApiError::bad_request("invalid_correlation", error.to_string()))?;

    let mut log_query = projection::LogQuery::for_project(&request.project);
    log_query.environment = request.environment.clone();
    log_query.start_time = request.time_range.start.clone();
    log_query.end_time = request.time_range.end.clone();
    log_query.trace_id = request.trace_id.clone();
    log_query.span_id = request.span_id.clone();
    log_query.service_name = request.service.clone();
    log_query.attribute_equals = attributes.clone();
    log_query.limit = request.limit;
    let logs = state
        .projections
        .query_logs(&log_query)
        .map_err(|error| ApiError::bad_request("invalid_correlation", error.to_string()))?
        .records;

    let mut metric_query = projection::MetricQuery::for_project(&request.project);
    metric_query.environment = request.environment.clone();
    metric_query.start_time = request.time_range.start.clone();
    metric_query.end_time = request.time_range.end.clone();
    metric_query.attribute_equals = attributes.clone();
    if let Some(service) = &request.service {
        metric_query
            .resource_equals
            .insert("service.name".into(), service.clone());
    }
    metric_query.limit = projection::MAX_METRIC_QUERY_LIMIT;
    let mut metrics = state
        .projections
        .query_metrics(&metric_query)
        .map_err(|error| ApiError::bad_request("invalid_correlation", error.to_string()))?
        .series;
    if request.trace_id.is_some() || request.span_id.is_some() {
        metrics.retain(|series| {
            series.points.iter().any(|point| {
                point.exemplars.iter().any(|exemplar| {
                    request
                        .trace_id
                        .as_ref()
                        .is_none_or(|trace_id| &exemplar.trace_id == trace_id)
                        && request
                            .span_id
                            .as_ref()
                            .is_none_or(|span_id| &exemplar.span_id == span_id)
                })
            })
        });
    }
    metrics.truncate(request.limit);

    let mut warnings = Vec::new();
    let traces = if let Some(trace_id) = &request.trace_id {
        match state
            .projections
            .get_trace(&request.project, trace_id)
            .map_err(|error| ApiError::bad_request("invalid_correlation", error.to_string()))?
        {
            Some(trace) => vec![trace],
            None => {
                warnings.push(format!("trace `{trace_id}` was not found"));
                Vec::new()
            }
        }
    } else {
        let mut trace_query = projection::TraceQuery::for_project(&request.project);
        trace_query.environment = request.environment.clone();
        trace_query.start_time_unix_nano = request
            .time_range
            .start
            .as_deref()
            .map(parse_query_time_nanos)
            .transpose()?;
        trace_query.end_time_unix_nano = request
            .time_range
            .end
            .as_deref()
            .map(parse_query_time_nanos)
            .transpose()?;
        trace_query.service = request.service.clone();
        trace_query.attributes = attributes;
        trace_query.limit = request.limit;
        let mut traces = state
            .projections
            .query_traces(&trace_query)
            .map_err(|error| ApiError::bad_request("invalid_correlation", error.to_string()))?
            .traces;
        if let Some(span_id) = &request.span_id {
            traces.retain(|trace| trace.spans.iter().any(|span| &span.span_id == span_id));
        }
        traces
    };
    let watermark = [
        projection::PROJECTION_LOGGING_STORE,
        projection::PROJECTION_METRIC_STORE,
        projection::PROJECTION_TRACE_STORE,
    ]
    .into_iter()
    .map(|projection| state.projections.current_cursor(projection))
    .collect::<Result<Vec<_>>>()
    .map_err(|error| ApiError::internal(error.to_string()))?
    .into_iter()
    .max()
    .unwrap_or(0);
    Ok(Json(CorrelationResponseV1 {
        logs,
        metrics,
        traces,
        watermark,
        partial: !warnings.is_empty(),
        warnings,
    }))
}
