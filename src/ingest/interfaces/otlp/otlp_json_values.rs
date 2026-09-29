//! Reading OTLP/JSON's arrays, strings, nanosecond times and ids.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::Value;

pub(super) fn array<'a>(value: &'a Value, camel: &str, snake: &str) -> &'a [Value] {
    value
        .get(camel)
        .or_else(|| value.get(snake))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub(super) fn string<'a>(value: &'a Value, camel: &str, snake: &str) -> Option<&'a str> {
    value
        .get(camel)
        .or_else(|| value.get(snake))
        .and_then(Value::as_str)
}

pub(super) fn nanos(value: &Value, camel: &str, snake: &str) -> u64 {
    value
        .get(camel)
        .or_else(|| value.get(snake))
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
        .unwrap_or(0)
}

pub(super) fn scalar_value_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

pub(super) fn id_string(value: Option<&Value>) -> Option<String> {
    let source = value?.as_str()?;
    if source.is_empty() {
        return None;
    }
    if source.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Some(source.to_ascii_lowercase());
    }
    BASE64
        .decode(source)
        .ok()
        .map(hex::encode)
        .or_else(|| Some(source.to_string()))
}

pub(super) fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}
