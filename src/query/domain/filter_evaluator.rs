//! Evaluating a filter expression against an event's fields.

use anyhow::{Context, Result};
use regex::Regex;

use crate::query::domain::filter_expression::QueryExpressionV1;

pub fn evaluate_filter(filter: &QueryExpressionV1, document: &serde_json::Value) -> Result<bool> {
    match filter {
        QueryExpressionV1::And { args } => {
            for arg in args {
                if !evaluate_filter(arg, document)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        QueryExpressionV1::Or { args } => {
            for arg in args {
                if evaluate_filter(arg, document)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        QueryExpressionV1::Not { arg } => Ok(!evaluate_filter(arg, document)?),
        QueryExpressionV1::Eq { field, value } => Ok(field_value(document, field) == Some(value)),
        QueryExpressionV1::In { field, values } => Ok(field_value(document, field)
            .is_some_and(|actual| values.iter().any(|value| value == actual))),
        QueryExpressionV1::Exists { field } => {
            Ok(field_value(document, field).is_some_and(|value| !value.is_null()))
        }
        QueryExpressionV1::Range {
            field,
            gt,
            gte,
            lt,
            lte,
        } => {
            let Some(actual) = field_value(document, field) else {
                return Ok(false);
            };
            Ok(gt
                .as_ref()
                .is_none_or(|bound| compare(actual, bound).is_some_and(|value| value > 0))
                && gte
                    .as_ref()
                    .is_none_or(|bound| compare(actual, bound).is_some_and(|value| value >= 0))
                && lt
                    .as_ref()
                    .is_none_or(|bound| compare(actual, bound).is_some_and(|value| value < 0))
                && lte
                    .as_ref()
                    .is_none_or(|bound| compare(actual, bound).is_some_and(|value| value <= 0)))
        }
        QueryExpressionV1::Text { field, value } => Ok(field_value(document, field)
            .and_then(value_text)
            .is_some_and(|actual| actual.to_lowercase().contains(&value.to_lowercase()))),
        QueryExpressionV1::Regex { field, pattern } => {
            let regex = Regex::new(pattern).context("invalid regex pattern")?;
            Ok(field_value(document, field)
                .and_then(value_text)
                .is_some_and(|actual| regex.is_match(&actual)))
        }
    }
}

fn field_value<'a>(document: &'a serde_json::Value, field: &str) -> Option<&'a serde_json::Value> {
    if let Some(value) = document.get(field) {
        return Some(value);
    }
    let (first, remainder) = field.split_once('.')?;
    let child = document.get(first)?;
    if let Some(value) = child.get(remainder) {
        return Some(value);
    }
    remainder
        .split('.')
        .try_fold(child, |value, part| value.get(part))
}

fn value_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn compare(left: &serde_json::Value, right: &serde_json::Value) -> Option<i8> {
    if let (Some(left), Some(right)) = (left.as_f64(), right.as_f64()) {
        return left.partial_cmp(&right).map(|ordering| match ordering {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        });
    }
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return Some(match left.cmp(right) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        });
    }
    None
}
