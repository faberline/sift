//! Building the metric query a PromQL query reads, and evaluating a range query
//! over the series it returns.

use anyhow::Result;

use crate::event::AttributeValue;
use crate::query::domain::promql::{ParsedPromQuery, PromFunction};
use crate::query::domain::promql_evaluator::{
    prom_latest_values, prom_nanos_to_seconds, prom_number, prom_rate_values, prom_series_labels,
    PromStepAggregate,
};
use crate::{projection, ApiError};

pub(in crate::query) fn prom_metric_query(
    project: &str,
    environment: Option<String>,
    parsed: &ParsedPromQuery,
) -> Result<projection::MetricQuery, ApiError> {
    let mut query = projection::MetricQuery::for_project(project);
    query.environment = environment;
    query.name = Some(parsed.metric.clone());
    query.limit = projection::MAX_METRIC_QUERY_LIMIT;
    for (name, value) in &parsed.labels {
        if name.starts_with("service.") || name.starts_with("cloud.") || name.starts_with("k8s.") {
            query.resource_equals.insert(name.clone(), value.clone());
        } else {
            query
                .attribute_equals
                .insert(name.clone(), AttributeValue::String(value.clone()));
        }
    }
    Ok(query)
}

pub(in crate::query) fn prom_range_result(
    series: &[projection::MetricSeriesResultV1],
    function: PromFunction,
    start: i64,
    end: i64,
    step: i64,
) -> Vec<serde_json::Value> {
    let evaluation_count = ((i128::from(end) - i128::from(start)) / i128::from(step) + 1) as usize;
    let evaluation_times = (0..evaluation_count)
        .map(|index| {
            (i128::from(start) + i128::from(step) * index as i128)
                .try_into()
                .expect("evaluation time was validated")
        })
        .collect::<Vec<_>>();
    if function == PromFunction::Raw || function == PromFunction::Rate {
        return series
            .iter()
            .filter_map(|series| {
                let step_values = if function == PromFunction::Rate {
                    prom_rate_values(&series.points, &evaluation_times)
                } else {
                    prom_latest_values(&series.points, &evaluation_times)
                };
                let values = evaluation_times
                    .iter()
                    .zip(step_values)
                    .filter_map(|(evaluation_time, value)| {
                        value.map(|value| {
                            serde_json::json!([
                                prom_nanos_to_seconds(*evaluation_time),
                                prom_number(value)
                            ])
                        })
                    })
                    .collect::<Vec<_>>();
                if values.is_empty() {
                    return None;
                }
                Some(serde_json::json!({
                    "metric": prom_series_labels(series),
                    "values": values
                }))
            })
            .collect();
    }
    let mut aggregates = vec![PromStepAggregate::default(); evaluation_times.len()];
    for series in series {
        for (aggregate, value) in aggregates
            .iter_mut()
            .zip(prom_latest_values(&series.points, &evaluation_times))
        {
            if let Some(value) = value {
                aggregate.observe(value);
            }
        }
    }
    let values = evaluation_times
        .iter()
        .zip(aggregates)
        .filter_map(|(evaluation_time, aggregate)| {
            aggregate.value(function).map(|value| {
                serde_json::json!([prom_nanos_to_seconds(*evaluation_time), prom_number(value)])
            })
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        Vec::new()
    } else {
        vec![serde_json::json!({"metric": {}, "values": values})]
    }
}
