//! OTLP/JSON resources, scopes and attributes as event attributes.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::ingest::interfaces::otlp::otlp_json_values::{array, scalar_string, string};
use crate::{AttributeValue, InstrumentationScope};

pub(super) fn json_resource(resource: Option<&Value>) -> BTreeMap<String, String> {
    let mut output = BTreeMap::new();
    if let Some(resource) = resource {
        for (key, value) in json_key_values(resource.get("attributes")) {
            if let Some(value) = scalar_string(&value) {
                output.insert(key, value);
            }
        }
    }
    if output.is_empty() {
        output.insert("service.name".into(), "unknown".into());
    }
    output
}

pub(super) fn json_scope(
    scope: Option<&Value>,
    schema_url: Option<&str>,
) -> Option<InstrumentationScope> {
    let scope = scope?;
    Some(InstrumentationScope {
        name: string(scope, "name", "name")
            .unwrap_or("unknown")
            .to_string(),
        version: string(scope, "version", "version").map(str::to_string),
        attributes: json_attributes(scope.get("attributes")),
        schema_url: schema_url
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

fn json_key_values(value: Option<&Value>) -> Vec<(String, Value)> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let key = entry.get("key")?.as_str()?.to_string();
            let value = entry.get("value").and_then(json_any_value)?;
            Some((key, value))
        })
        .collect()
}

pub(super) fn json_attributes(value: Option<&Value>) -> BTreeMap<String, AttributeValue> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            Some((
                entry.get("key")?.as_str()?.to_string(),
                json_attribute_value(entry.get("value")?)?,
            ))
        })
        .collect()
}

pub(super) fn json_attribute_value(value: &Value) -> Option<AttributeValue> {
    if let Some(value) = value.get("stringValue").and_then(Value::as_str) {
        return Some(AttributeValue::String(value.to_string()));
    }
    if let Some(value) = value.get("boolValue").and_then(Value::as_bool) {
        return Some(AttributeValue::Bool(value));
    }
    if let Some(value) = value.get("intValue") {
        return value
            .as_i64()
            .or_else(|| value.as_str()?.parse().ok())
            .map(AttributeValue::Int);
    }
    if let Some(value) = value.get("doubleValue") {
        return value
            .as_f64()
            .or_else(|| value.as_str()?.parse().ok())
            .map(AttributeValue::Double);
    }
    if let Some(value) = value.get("bytesValue").and_then(Value::as_str) {
        return Some(AttributeValue::Bytes(value.to_string()));
    }
    if let Some(value) = value.get("arrayValue") {
        return Some(AttributeValue::Array(
            array(value, "values", "values")
                .iter()
                .filter_map(json_attribute_value)
                .collect(),
        ));
    }
    if let Some(value) = value.get("kvlistValue") {
        return Some(AttributeValue::Map(
            value
                .get("values")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    Some((
                        entry.get("key")?.as_str()?.to_string(),
                        json_attribute_value(entry.get("value")?)?,
                    ))
                })
                .collect(),
        ));
    }
    (!value.is_null()).then(|| json_to_attribute(value.clone()))
}

pub(super) fn json_any_value(value: &Value) -> Option<Value> {
    if !value.is_object() {
        return Some(value.clone());
    }
    for key in ["stringValue", "boolValue", "intValue", "doubleValue"] {
        if let Some(value) = value.get(key) {
            return Some(value.clone());
        }
    }
    if let Some(bytes) = value.get("bytesValue").and_then(Value::as_str) {
        return Some(json!({"bytesBase64": bytes}));
    }
    if let Some(array_value) = value.get("arrayValue") {
        return Some(Value::Array(
            array(array_value, "values", "values")
                .iter()
                .filter_map(json_any_value)
                .collect(),
        ));
    }
    if let Some(map) = value.get("kvlistValue") {
        return Some(Value::Object(
            json_key_values(map.get("values")).into_iter().collect(),
        ));
    }
    Some(value.clone())
}

pub(super) fn json_to_attribute(value: Value) -> AttributeValue {
    match value {
        Value::String(value) => AttributeValue::String(value),
        Value::Bool(value) => AttributeValue::Bool(value),
        Value::Number(value) => value
            .as_i64()
            .map(AttributeValue::Int)
            .unwrap_or_else(|| AttributeValue::Double(value.as_f64().unwrap_or_default())),
        Value::Array(values) => {
            AttributeValue::Array(values.into_iter().map(json_to_attribute).collect())
        }
        Value::Object(values) => AttributeValue::Map(
            values
                .into_iter()
                .map(|(key, value)| (key, json_to_attribute(value)))
                .collect(),
        ),
        Value::Null => AttributeValue::String("null".into()),
    }
}
