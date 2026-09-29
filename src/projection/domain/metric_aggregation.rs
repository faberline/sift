//! Rollups over fixed windows, a series' semantic total by temporality, the
//! query aggregations, and histogram merging.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::projection::domain::metric::{
    MetricAggregation, MetricHistogramV1, MetricPointV1, MetricRollupV1, ROLLUP_WINDOWS_SECONDS,
};
use crate::MetricTemporality;

pub(in crate::projection) fn make_rollups(points: &[MetricPointV1]) -> Result<Vec<MetricRollupV1>> {
    let mut rollups = Vec::new();
    for window_seconds in ROLLUP_WINDOWS_SECONDS {
        let window_nanos = (window_seconds as i64) * 1_000_000_000;
        let mut groups = BTreeMap::<i64, Vec<&MetricPointV1>>::new();
        for point in points.iter().filter(|point| !point.stale) {
            let start = point.time_unix_nano.div_euclid(window_nanos) * window_nanos;
            groups.entry(start).or_default().push(point);
        }
        for (start, points) in groups {
            let values = points.iter().map(|point| point.value).collect::<Vec<_>>();
            let histogram =
                merge_histogram_refs(points.iter().filter_map(|point| point.histogram.as_ref()))?;
            let mut exemplars = points
                .iter()
                .flat_map(|point| point.exemplars.iter().cloned())
                .collect::<Vec<_>>();
            exemplars.sort_by(|left, right| {
                left.trace_id
                    .cmp(&right.trace_id)
                    .then_with(|| left.span_id.cmp(&right.span_id))
            });
            exemplars.dedup_by(|left, right| {
                left.trace_id == right.trace_id && left.span_id == right.span_id
            });
            rollups.push(MetricRollupV1 {
                window_seconds,
                start_time_unix_nano: start,
                end_time_unix_nano: start.saturating_add(window_nanos),
                point_count: points.len() as u64,
                sum: values.iter().sum(),
                min: values.iter().copied().fold(f64::INFINITY, f64::min),
                max: values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                last: points.last().expect("non-empty rollup").value,
                histogram,
                exemplars,
            });
        }
    }
    rollups.sort_by(|left, right| {
        left.window_seconds
            .cmp(&right.window_seconds)
            .then_with(|| left.start_time_unix_nano.cmp(&right.start_time_unix_nano))
    });
    Ok(rollups)
}

pub(in crate::projection) fn semantic_total(
    temporality: MetricTemporality,
    points: &[MetricPointV1],
) -> (f64, u64) {
    match temporality {
        MetricTemporality::Gauge => (points.last().map_or(0.0, |point| point.value), 0),
        MetricTemporality::Delta => (points.iter().map(|point| point.value).sum(), 0),
        MetricTemporality::Cumulative => {
            let mut total = 0.0;
            let mut resets = 0;
            for window in points.windows(2) {
                if window[1].value >= window[0].value {
                    total += window[1].value - window[0].value;
                } else {
                    resets += 1;
                    total += window[1].value.max(0.0);
                }
            }
            (total, resets)
        }
    }
}

pub(in crate::projection) fn aggregate(
    aggregation: MetricAggregation,
    semantic_total: f64,
    points: &[MetricPointV1],
) -> Option<f64> {
    match aggregation {
        MetricAggregation::Raw => Some(semantic_total),
        MetricAggregation::Sum => Some(points.iter().map(|point| point.value).sum()),
        MetricAggregation::Avg => {
            Some(points.iter().map(|point| point.value).sum::<f64>() / points.len() as f64)
        }
        MetricAggregation::Min => points.iter().map(|point| point.value).reduce(f64::min),
        MetricAggregation::Max => points.iter().map(|point| point.value).reduce(f64::max),
        MetricAggregation::Count => Some(points.len() as f64),
        MetricAggregation::Rate => {
            let duration = points
                .last()
                .zip(points.first())
                .map(|(last, first)| last.time_unix_nano - first.time_unix_nano)
                .unwrap_or(0);
            (duration > 0).then_some(semantic_total / (duration as f64 / 1_000_000_000.0))
        }
    }
}

pub(in crate::projection) fn merge_histograms(
    points: &[MetricPointV1],
) -> Result<Option<MetricHistogramV1>> {
    merge_histogram_refs(points.iter().filter_map(|point| point.histogram.as_ref()))
}

fn merge_histogram_refs<'a>(
    histograms: impl Iterator<Item = &'a MetricHistogramV1>,
) -> Result<Option<MetricHistogramV1>> {
    let mut merged: Option<MetricHistogramV1> = None;
    for histogram in histograms {
        match &mut merged {
            Some(merged) => merged.merge(histogram)?,
            None => {
                histogram.validate()?;
                merged = Some(histogram.clone());
            }
        }
    }
    Ok(merged)
}

pub(super) fn merge_counts(left: &mut [u64], right: &[u64]) {
    for (left, right) in left.iter_mut().zip(right) {
        *left = left.saturating_add(*right);
    }
}

pub(super) fn optional_min(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    }
}

pub(super) fn optional_max(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    }
}
