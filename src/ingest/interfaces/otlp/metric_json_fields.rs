//! Reject malformed present fields instead of replacing them with defaults.
use anyhow::{anyhow, bail, Result};
use serde_json::Value;

pub(super) fn field<'a>(value: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    value.get(camel).or_else(|| value.get(snake))
}
pub(super) fn unsigned(value: &Value, camel: &str, snake: &str) -> Result<Option<u64>> {
    field(value, camel, snake)
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_u64()
                .or_else(|| v.as_str()?.parse().ok())
                .ok_or_else(|| anyhow!("{camel} must be an unsigned integer"))
        })
        .transpose()
}
pub(super) fn signed(value: &Value, camel: &str, snake: &str) -> Result<Option<i64>> {
    field(value, camel, snake)
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_i64()
                .or_else(|| v.as_str()?.parse().ok())
                .ok_or_else(|| anyhow!("{camel} must be a signed integer"))
        })
        .transpose()
}
pub(super) fn number(value: &Value, camel: &str, snake: &str) -> Result<Option<f64>> {
    field(value, camel, snake)
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_f64()
                .or_else(|| v.as_str()?.parse().ok())
                .filter(|v| v.is_finite())
                .ok_or_else(|| anyhow!("{camel} must be a finite number"))
        })
        .transpose()
}
pub(super) fn boolean(value: &Value, camel: &str, snake: &str) -> Result<Option<bool>> {
    field(value, camel, snake)
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| anyhow!("{camel} must be a boolean"))
        })
        .transpose()
}
pub(super) fn array<'a>(value: &'a Value, camel: &str, snake: &str) -> Result<&'a [Value]> {
    match field(value, camel, snake).filter(|v| !v.is_null()) {
        None => Ok(&[]),
        Some(v) => v
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| anyhow!("{camel} must be an array")),
    }
}
pub(super) fn point_number(value: &Value) -> Result<Option<f64>> {
    let double = number(value, "asDouble", "as_double")?;
    let integer = signed(value, "asInt", "as_int")?;
    if double.is_some() && integer.is_some() {
        bail!("metric point must have one numeric value");
    }
    Ok(double.or_else(|| integer.map(|v| v as f64)))
}
