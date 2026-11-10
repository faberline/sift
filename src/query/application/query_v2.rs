//! One execution path for V2 HTTP, CLI, agent, and alert queries.
use crate::access::application::project_authorization::authorize_project_read;
use crate::app::{interfaces::http::api_error::ApiError, service_state::ServiceState};
use crate::query::domain::{
    metric_evaluator::evaluate,
    metric_expression::{ExpressionParser, Value},
    promql_evaluator::prom_number,
};
use crate::shared_kernel::query_response::QueryResponseV1;
pub use crate::shared_kernel::query_v2::{QueryRequestV2, QueryResponseV2, QuerySignalV2};
use chrono::{DateTime, Utc};
use std::{sync::Arc, time::Instant};

pub(crate) struct QueryServiceV2 {
    parser: Arc<dyn ExpressionParser>,
}
impl QueryServiceV2 {
    pub fn new(parser: Arc<dyn ExpressionParser>) -> Self {
        Self { parser }
    }
    pub fn execute(
        &self,
        state: &ServiceState,
        principal: Option<&service_auth::RoleMapPrincipal>,
        request: &QueryRequestV2,
    ) -> Result<QueryResponseV2, ApiError> {
        let started = Instant::now();
        if request.version != 2 || request.project.trim().is_empty() {
            return Err(ApiError::bad_request(
                "invalid_query",
                "V2 requires version 2 and a project",
            ));
        }
        authorize_project_read(principal, &request.project)?;
        state
            .journal()
            .ensure_queryable()
            .map_err(|e| ApiError::temporarily_unavailable(e.to_string()))?;
        let response = match &request.signal {
            QuerySignalV2::LogsCount { filter } => {
                super::log_count::count_logs(state, request, filter.as_deref())?
            }
            QuerySignalV2::Metrics { expression } => {
                let expr = self
                    .parser
                    .parse(expression)
                    .map_err(|e| ApiError::bad_request("unsupported_query", e))?;
                let time = |value: &str| {
                    DateTime::parse_from_rfc3339(value)
                        .ok()
                        .and_then(|t| t.timestamp_nanos_opt())
                        .ok_or_else(|| {
                            ApiError::bad_request(
                                "invalid_query",
                                "query time must be RFC3339 within nanosecond range",
                            )
                        })
                };
                let end = request
                    .time_range
                    .end
                    .as_deref()
                    .map(time)
                    .transpose()?
                    .unwrap_or_else(|| Utc::now().timestamp_nanos_opt().unwrap_or(i64::MAX));
                let start = request
                    .time_range
                    .start
                    .as_deref()
                    .map(time)
                    .transpose()?
                    .unwrap_or(end);
                let step = request.step_seconds.unwrap_or(30);
                if start > end || step == 0 || step > 3600 {
                    return Err(ApiError::bad_request(
                        "invalid_query",
                        "metric range or step is invalid",
                    ));
                }
                let step = (step * 1_000_000_000) as i64;
                let steps = end
                    .checked_sub(start)
                    .and_then(|v| v.checked_div(step))
                    .and_then(|v| v.checked_add(1))
                    .filter(|v| *v <= 11_000)
                    .ok_or_else(|| {
                        ApiError::bad_request(
                            "query_limit",
                            "metric evaluation exceeds 11000 steps",
                        )
                    })?;
                let (series, watermark, scanned) =
                    super::metric_source::load_metrics(state, request, &expr, start, end)?;
                if series.len().saturating_mul(steps as usize) > 1_000_000 {
                    return Err(ApiError::bad_request(
                        "query_limit",
                        "metric evaluation exceeds one million series steps",
                    ));
                }
                let mut results: std::collections::BTreeMap<_, Vec<serde_json::Value>> =
                    std::collections::BTreeMap::new();
                let mut scalar = None;
                for index in 0..steps {
                    let at = start + index * step;
                    match evaluate(&expr, &series, at)
                        .map_err(|e| ApiError::bad_request("invalid_query", e))?
                    {
                        Value::Scalar(value) => {
                            scalar = Some(serde_json::json!([at as f64 / 1e9, prom_number(value)]))
                        }
                        Value::Vector(vector) => {
                            for (labels, value) in vector {
                                results
                                    .entry(labels)
                                    .or_default()
                                    .push(serde_json::json!([at as f64 / 1e9, prom_number(value)]));
                            }
                        }
                    }
                }
                let returned = results.len();
                let data = if let Some(value) = scalar {
                    if steps != 1 {
                        return Err(ApiError::bad_request(
                            "unsupported_query",
                            "scalar range results are not supported",
                        ));
                    }
                    serde_json::json!({"resultType":"scalar","result":value})
                } else if request.time_range.start.is_some() {
                    serde_json::json!({"resultType":"matrix","result":results.into_iter().map(|(metric, values)|
                        serde_json::json!({"metric":metric,"values":values})).collect::<Vec<_>>()})
                } else {
                    serde_json::json!({"resultType":"vector","result":results.into_iter().map(|(metric, mut values)|
                        serde_json::json!({"metric":metric,"value":values.remove(0)})).collect::<Vec<_>>()})
                };
                QueryResponseV1::complete(
                    data,
                    None,
                    watermark,
                    scanned,
                    returned,
                    started.elapsed(),
                )
            }
        };
        Ok(QueryResponseV2 {
            version: 2,
            response,
        })
    }
}
