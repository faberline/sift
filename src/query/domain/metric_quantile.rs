//! Classic histogram quantiles over cumulative buckets.
use super::metric_expression::{Labels, Value};
use std::collections::BTreeMap;

fn quantile(q: f64, mut buckets: Vec<(f64, f64)>) -> f64 {
    if q.is_nan() {
        return f64::NAN;
    }
    if q < 0.0 {
        return f64::NEG_INFINITY;
    }
    if q > 1.0 {
        return f64::INFINITY;
    }
    buckets.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut combined: Vec<(f64, f64)> = Vec::new();
    for bucket in buckets {
        if let Some(last) = combined.last_mut().filter(|last| last.0 == bucket.0) {
            last.1 += bucket.1;
        } else {
            combined.push(bucket);
        }
    }
    if combined.len() < 2 || combined.last().is_none_or(|b| b.0 != f64::INFINITY) {
        return f64::NAN;
    }
    let mut previous = 0.0;
    for bucket in &mut combined {
        if (bucket.1 - previous).abs() <= 1e-12 * (bucket.1.abs() + previous.abs())
            || bucket.1 < previous
        {
            bucket.1 = previous;
        }
        previous = bucket.1;
    }
    if previous <= 0.0 {
        return f64::NAN;
    }
    let mut rank = q * previous;
    let index = combined
        .iter()
        .take(combined.len() - 1)
        .position(|b| b.1 >= rank)
        .unwrap_or(combined.len() - 1);
    if index == combined.len() - 1 {
        return combined[index - 1].0;
    }
    if index == 0 && combined[0].0 <= 0.0 {
        return combined[0].0;
    }
    let mut start = 0.0;
    let (end, mut count) = combined[index];
    if index > 0 {
        start = combined[index - 1].0;
        count -= combined[index - 1].1;
        rank -= combined[index - 1].1;
    }
    start + (end - start) * rank / count
}

pub(crate) fn histogram_quantile(q: Value, buckets: Value) -> Result<Value, String> {
    let Value::Scalar(q) = q else {
        return Err("histogram_quantile requires a scalar quantile".into());
    };
    let Value::Vector(buckets) = buckets else {
        return Err("histogram_quantile requires a bucket vector".into());
    };
    let mut groups: BTreeMap<Labels, Vec<(f64, f64)>> = BTreeMap::new();
    for (mut labels, count) in buckets {
        let Some(le) = labels.remove("le") else {
            continue;
        };
        let bound = if le == "+Inf" {
            f64::INFINITY
        } else {
            le.parse().map_err(|_| "invalid histogram le label")?
        };
        if f64::is_nan(bound) || !count.is_finite() || count < 0.0 {
            return Err("invalid histogram bucket".into());
        }
        labels.remove("__name__");
        groups.entry(labels).or_default().push((bound, count));
    }
    Ok(Value::Vector(
        groups
            .into_iter()
            .map(|(labels, buckets)| (labels, quantile(q, buckets)))
            .collect(),
    ))
}
