//! Evaluating PromQL over metric series: the lookback and range budgets, series
//! labels, latest values, rates and step aggregates.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::event::AttributeValue;
use crate::query::domain::promql::PromFunction;
use crate::{projection, ApiError};

pub(in crate::query) fn ensure_complete_prom_metric_page(
    page: &projection::MetricPage,
) -> Result<(), ApiError> {
    if page.has_more {
        return Err(ApiError::bad_request(
            "bad_data",
            format!(
                "Prometheus selector matches more than {} series; narrow the selector",
                projection::MAX_METRIC_QUERY_LIMIT
            ),
        ));
    }
    Ok(())
}

pub(in crate::query) fn ensure_prom_range_work_budget(
    series_count: usize,
    evaluation_count: usize,
) -> Result<(), ApiError> {
    let work_samples = series_count
        .checked_mul(evaluation_count)
        .ok_or_else(|| ApiError::bad_request("bad_data", "query_range sample budget overflowed"))?;
    if work_samples > PROM_MAX_RANGE_WORK_SAMPLES {
        return Err(ApiError::bad_request(
            "bad_data",
            format!(
                "query_range would evaluate {work_samples} series samples; the limit is {PROM_MAX_RANGE_WORK_SAMPLES}"
            ),
        ));
    }
    Ok(())
}

pub(in crate::query) fn prom_series_labels(
    series: &projection::MetricSeriesResultV1,
) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::from([("__name__".into(), series.name.clone())]);
    labels.extend(
        series
            .resource
            .iter()
            .filter(|(name, _)| name.as_str() != "telemetry.sdk.name")
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    for (name, value) in &series.attributes {
        let value = match value {
            AttributeValue::String(value) => value.clone(),
            AttributeValue::Bool(value) => value.to_string(),
            AttributeValue::Int(value) => value.to_string(),
            AttributeValue::Double(value) => prom_number(*value),
            _ => continue,
        };
        labels.insert(name.clone(), value);
    }
    labels
}

pub(in crate::query) fn prom_number(value: f64) -> String {
    if value.is_nan() {
        "NaN".into()
    } else if value == f64::INFINITY {
        "+Inf".into()
    } else if value == f64::NEG_INFINITY {
        "-Inf".into()
    } else {
        value.to_string()
    }
}

pub(in crate::query) fn prom_aggregate(function: PromFunction, values: &[f64]) -> Option<f64> {
    match function {
        PromFunction::Sum => Some(values.iter().sum()),
        PromFunction::Avg => {
            (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
        }
        PromFunction::Min => values.iter().copied().reduce(f64::min),
        PromFunction::Max => values.iter().copied().reduce(f64::max),
        PromFunction::Count => Some(values.len() as f64),
        _ => None,
    }
}

pub(in crate::query) const PROM_LOOKBACK_NANOS: i64 = 300_000_000_000;
pub(in crate::query) const PROM_MAX_RANGE_EVALUATIONS: i64 = 11_000;
const PROM_MAX_RANGE_WORK_SAMPLES: usize = 1_000_000;

pub(in crate::query) fn prom_nanos_to_seconds(nanos: i64) -> f64 {
    nanos as f64 / 1_000_000_000.0
}

pub(in crate::query) fn prom_latest_values(
    points: &[projection::MetricPointV1],
    evaluation_times: &[i64],
) -> Vec<Option<f64>> {
    let mut next_point = 0usize;
    evaluation_times
        .iter()
        .map(|evaluation_time| {
            while next_point < points.len() && points[next_point].time_unix_nano <= *evaluation_time
            {
                next_point += 1;
            }
            let point = points.get(next_point.checked_sub(1)?)?;
            let oldest_visible = evaluation_time.checked_sub(PROM_LOOKBACK_NANOS);
            (!point.stale
                && oldest_visible
                    .is_none_or(|oldest_visible| point.time_unix_nano > oldest_visible))
            .then_some(point.value)
        })
        .collect()
}

pub(in crate::query) fn prom_rate_values(
    points: &[projection::MetricPointV1],
    evaluation_times: &[i64],
) -> Vec<Option<f64>> {
    let mut counter_prefix = Vec::with_capacity(points.len());
    let mut previous = None;
    let mut cumulative = 0.0;
    for point in points {
        if point.stale {
            previous = None;
            cumulative = 0.0;
        } else if let Some(previous_value) = previous {
            cumulative += if point.value >= previous_value {
                point.value - previous_value
            } else {
                point.value.max(0.0)
            };
            previous = Some(point.value);
        } else {
            previous = Some(point.value);
        }
        counter_prefix.push(cumulative);
    }

    let mut next_point = 0usize;
    let mut epoch_start = 0usize;
    let mut window_start = 0usize;
    evaluation_times
        .iter()
        .map(|evaluation_time| {
            while next_point < points.len() && points[next_point].time_unix_nano <= *evaluation_time
            {
                if points[next_point].stale {
                    epoch_start = next_point + 1;
                    window_start = epoch_start;
                }
                next_point += 1;
            }
            if next_point <= epoch_start {
                return None;
            }
            window_start = window_start.max(epoch_start);
            if let Some(oldest_visible) = evaluation_time.checked_sub(PROM_LOOKBACK_NANOS) {
                while window_start < next_point
                    && points[window_start].time_unix_nano <= oldest_visible
                {
                    window_start += 1;
                }
            }
            if window_start >= next_point {
                return None;
            }
            let last = next_point - 1;
            let elapsed = points[last].time_unix_nano - points[window_start].time_unix_nano;
            if elapsed <= 0 {
                return None;
            }
            let delta = counter_prefix[last] - counter_prefix[window_start];
            Some(delta / (elapsed as f64 / 1_000_000_000.0))
        })
        .collect()
}

#[derive(Clone, Default)]
pub(in crate::query) struct PromStepAggregate {
    count: usize,
    sum: f64,
    min: Option<f64>,
    max: Option<f64>,
}

impl PromStepAggregate {
    pub(in crate::query) fn observe(&mut self, value: f64) {
        self.count += 1;
        self.sum += value;
        self.min = Some(self.min.map_or(value, |current| current.min(value)));
        self.max = Some(self.max.map_or(value, |current| current.max(value)));
    }

    pub(in crate::query) fn value(&self, function: PromFunction) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        match function {
            PromFunction::Sum => Some(self.sum),
            PromFunction::Avg => Some(self.sum / self.count as f64),
            PromFunction::Min => self.min,
            PromFunction::Max => self.max,
            PromFunction::Count => Some(self.count as f64),
            _ => None,
        }
    }
}

#[cfg(test)]
mod prometheus_evaluator_tests {
    use super::*;
    use crate::query::interfaces::http::prom_query_params::nanos_rfc3339;

    fn point(time_unix_nano: i64, value: f64) -> projection::MetricPointV1 {
        projection::MetricPointV1 {
            cursor: 1,
            event_id: format!("point-{time_unix_nano}"),
            occurred_at: nanos_rfc3339(time_unix_nano).unwrap(),
            time_unix_nano,
            value,
            stale: false,
            histogram: None,
            exemplars: Vec::new(),
        }
    }

    #[test]
    fn minimum_nanosecond_has_no_false_lookback_boundary() {
        assert_eq!(
            prom_latest_values(&[point(i64::MIN, 8.0)], &[i64::MIN]),
            vec![Some(8.0)]
        );
        assert!(prom_rate_values(
            &[point(i64::MIN, 1.0), point(i64::MIN + 1, 2.0)],
            &[i64::MIN + 1]
        )[0]
        .is_some());
    }
}
