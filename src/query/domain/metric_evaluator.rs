//! Execute a bounded expression over Sift-owned metric samples.
use super::metric_binary::{binary, select_labels};
use super::metric_expression::{Expr, Labels, MatchOp, Selector, Series, Value};
use super::metric_quantile::histogram_quantile;
use super::metric_rate::{latest, window_change};
use std::collections::BTreeMap;

fn matches(selector: &Selector, labels: &Labels) -> Result<bool, String> {
    if selector
        .name
        .as_ref()
        .is_some_and(|name| labels.get("__name__") != Some(name))
    {
        return Ok(false);
    }
    for matcher in &selector.matchers {
        let value = labels.get(&matcher.name).map(String::as_str).unwrap_or("");
        let matched = match &matcher.op {
            MatchOp::Equal => value == matcher.value,
            MatchOp::NotEqual => value != matcher.value,
            MatchOp::Regex(regex) => regex.is_match(value),
            MatchOp::NotRegex(regex) => !regex.is_match(value),
        };
        if !matched {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn evaluate(expression: &Expr, series: &[Series], end: i64) -> Result<Value, String> {
    match expression {
        Expr::Scalar(value) => Ok(Value::Scalar(*value)),
        Expr::Select(selector)
        | Expr::RangeFunction {
            input: selector, ..
        } => {
            let mut out = Vec::new();
            for series in series {
                if !matches(selector, &series.labels)? {
                    continue;
                }
                let value = match expression {
                    Expr::RangeFunction { increase, .. } => window_change(
                        series,
                        end,
                        selector.window_nanos.ok_or("rate window is missing")?,
                        *increase,
                    )?,
                    _ => latest(&series.points, end),
                };
                if let Some(value) = value {
                    let mut labels = series.labels.clone();
                    if matches!(expression, Expr::RangeFunction { .. }) {
                        labels.remove("__name__");
                    }
                    out.push((labels, value));
                }
            }
            Ok(Value::Vector(out))
        }
        Expr::Negate(input) => match evaluate(input, series, end)? {
            Value::Scalar(value) => Ok(Value::Scalar(-value)),
            Value::Vector(vector) => Ok(Value::Vector(
                vector
                    .into_iter()
                    .map(|(mut labels, value)| {
                        labels.remove("__name__");
                        (labels, -value)
                    })
                    .collect(),
            )),
        },
        Expr::Aggregate { op, labels, input } => {
            let Value::Vector(vector) = evaluate(input, series, end)? else {
                return Err("aggregation requires a vector".into());
            };
            let mut groups: BTreeMap<Labels, Vec<f64>> = BTreeMap::new();
            for (input_labels, value) in vector {
                let key = labels
                    .as_ref()
                    .map_or_else(Labels::new, |s| select_labels(&input_labels, Some(s)));
                groups.entry(key).or_default().push(value);
            }
            Ok(Value::Vector(
                groups
                    .into_iter()
                    .map(|(labels, values)| {
                        let value = match op.as_str() {
                            "sum" => values.iter().sum(),
                            "avg" => values.iter().sum::<f64>() / values.len() as f64,
                            "count" => values.len() as f64,
                            "min" => values.into_iter().fold(f64::NAN, f64::min),
                            "max" => values.into_iter().fold(f64::NAN, f64::max),
                            _ => f64::NAN,
                        };
                        (labels, value)
                    })
                    .collect(),
            ))
        }
        Expr::Quantile { quantile, buckets } => histogram_quantile(
            evaluate(quantile, series, end)?,
            evaluate(buckets, series, end)?,
        ),
        Expr::Binary {
            op,
            left,
            right,
            matching,
            return_bool,
        } => binary(
            op,
            evaluate(left, series, end)?,
            evaluate(right, series, end)?,
            matching.as_ref(),
            *return_bool,
        ),
    }
}
