//! One-to-one label matching and scalar/vector arithmetic.
use super::metric_expression::{LabelSelection, Labels, Value};
use std::collections::BTreeMap;

pub(crate) fn select_labels(labels: &Labels, selection: Option<&LabelSelection>) -> Labels {
    labels
        .iter()
        .filter(|(name, _)| {
            selection.map_or(*name != "__name__", |s| {
                if s.include {
                    s.names.contains(name)
                } else {
                    *name != "__name__" && !s.names.contains(name)
                }
            })
        })
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

fn apply(op: &str, a: f64, b: f64, return_bool: bool) -> (f64, bool, bool) {
    let comparison = match op {
        "==" => Some(a == b),
        "!=" => Some(a != b),
        ">" => Some(a > b),
        "<" => Some(a < b),
        ">=" => Some(a >= b),
        "<=" => Some(a <= b),
        _ => None,
    };
    if let Some(ok) = comparison {
        return (
            if return_bool { f64::from(ok) } else { a },
            ok || return_bool,
            true,
        );
    }
    (
        match op {
            "+" => a + b,
            "-" => a - b,
            "*" => a * b,
            "/" => a / b,
            _ => f64::NAN,
        },
        true,
        false,
    )
}

pub(crate) fn binary(
    op: &str,
    left: Value,
    right: Value,
    matching: Option<&LabelSelection>,
    return_bool: bool,
) -> Result<Value, String> {
    match (left, right) {
        (Value::Scalar(a), Value::Scalar(b)) => Ok(Value::Scalar(apply(op, a, b, return_bool).0)),
        (Value::Vector(vector), Value::Scalar(scalar)) => Ok(Value::Vector(
            vector
                .into_iter()
                .filter_map(|(mut labels, value)| {
                    let (value, keep, comparison) = apply(op, value, scalar, return_bool);
                    if !comparison || return_bool {
                        labels.remove("__name__");
                    }
                    keep.then_some((labels, value))
                })
                .collect(),
        )),
        (Value::Scalar(scalar), Value::Vector(vector)) => Ok(Value::Vector(
            vector
                .into_iter()
                .filter_map(|(mut labels, vector_value)| {
                    let (value, keep, comparison) = apply(op, scalar, vector_value, return_bool);
                    if !comparison || return_bool {
                        labels.remove("__name__");
                    }
                    keep.then_some((
                        labels,
                        if comparison && !return_bool {
                            vector_value
                        } else {
                            value
                        },
                    ))
                })
                .collect(),
        )),
        (Value::Vector(left), Value::Vector(right)) => {
            let mut rhs = BTreeMap::new();
            for (labels, value) in right {
                if rhs
                    .insert(select_labels(&labels, matching), value)
                    .is_some()
                {
                    return Err("right operand has multiple series for a one-to-one match".into());
                }
            }
            let mut seen = BTreeMap::new();
            let mut out = Vec::new();
            for (mut labels, value) in left {
                let key = select_labels(&labels, matching);
                if seen.insert(key.clone(), ()).is_some() {
                    return Err("left operand has multiple series for a one-to-one match".into());
                }
                if let Some(other) = rhs.get(&key) {
                    let (value, keep, comparison) = apply(op, value, *other, return_bool);
                    if let Some(selection) = matching {
                        labels = select_labels(&labels, Some(selection));
                    }
                    if !comparison || return_bool || matching.is_some_and(|s| s.include) {
                        labels.remove("__name__");
                    }
                    if keep {
                        out.push((labels, value));
                    }
                }
            }
            Ok(Value::Vector(out))
        }
    }
}
