//! Counter windows, with reset handling and boundary extrapolation.
use super::metric_expression::{Sample, Series};
use crate::shared_kernel::event::MetricTemporality;

pub(crate) fn window_change(
    series: &Series,
    end: i64,
    window: i64,
    increase: bool,
) -> Result<Option<f64>, String> {
    let start = end.checked_sub(window).ok_or("metric range underflow")?;
    let points = series
        .points
        .iter()
        .filter(|p| p.time > start && p.time <= end && !p.stale)
        .collect::<Vec<_>>();
    if series.temporality == MetricTemporality::Delta {
        let mut total = 0.0;
        let mut previous_end = None;
        for point in &points {
            let point_start = point
                .start_time
                .and_then(|v| i64::try_from(v).ok())
                .filter(|v| *v < point.time)
                .ok_or("delta metric requires a valid start time")?;
            if point_start < start || previous_end.is_some_and(|v| v != point_start) {
                return Err("delta metric window has missing or overlapping intervals".into());
            }
            previous_end = Some(point.time);
            total += point.value;
        }
        if points.is_empty() {
            return Ok(None);
        }
        if previous_end != Some(end) || points[0].start_time != u64::try_from(start).ok() {
            return Err("delta metric does not cover the complete query window".into());
        }
        return Ok(Some(if increase {
            total
        } else {
            total / (window as f64 / 1e9)
        }));
    }
    if points.len() < 2 {
        return Ok(None);
    }
    let first = points[0];
    let last = points[points.len() - 1];
    if last.time <= first.time {
        return Err("counter samples must have increasing timestamps".into());
    }
    let mut change = last.value - first.value;
    for pair in points.windows(2) {
        if pair[0].time == pair[1].time {
            return Err("conflicting metric samples share a timestamp".into());
        }
        if pair[1].value < pair[0].value
            || pair[0]
                .start_time
                .zip(pair[1].start_time)
                .is_some_and(|(a, b)| a != b && b != 0)
        {
            change += pair[0].value;
        }
    }
    let sampled = (last.time - first.time) as f64 / 1e9;
    let spacing = sampled / (points.len() - 1) as f64;
    let mut before = (first.time - start) as f64 / 1e9;
    let mut after = (end - last.time) as f64 / 1e9;
    if change > 0.0 && first.value >= 0.0 {
        before = before.min(sampled * first.value / change);
    }
    if before >= spacing * 1.1 {
        before = spacing / 2.0;
    }
    if after >= spacing * 1.1 {
        after = spacing / 2.0;
    }
    let value = change * (sampled + before + after) / sampled;
    Ok(Some(if increase {
        value
    } else {
        value / (window as f64 / 1e9)
    }))
}

pub(crate) fn latest(points: &[Sample], end: i64) -> Option<f64> {
    points
        .iter()
        .rev()
        .find(|point| point.time <= end)
        .filter(|point| !point.stale && end.saturating_sub(point.time) <= 300_000_000_000)
        .map(|point| point.value)
}
