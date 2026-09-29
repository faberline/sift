//! OTLP protobuf resources, scopes, attributes and ids as event values.

use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::{json, Value};
use transport_otlp::proto as wire;
use transport_otlp::proto::{any_value, AnyValue};

use crate::ingest::interfaces::otlp::otlp_json_attributes::json_to_attribute;
use crate::ingest::interfaces::otlp::otlp_json_values::scalar_string;
use crate::{AttributeValue, InstrumentationScope};

pub(super) fn proto_resource(resource: Option<&wire::Resource>) -> BTreeMap<String, String> {
    let mut output: BTreeMap<String, String> = resource
        .map(|resource| {
            resource
                .attributes
                .iter()
                .filter_map(|entry| {
                    let value = entry.value.as_ref().map(proto_any_json)?;
                    Some((entry.key.clone(), scalar_string(&value)?))
                })
                .collect()
        })
        .unwrap_or_default();
    if output.is_empty() {
        output.insert("service.name".into(), "unknown".into());
    }
    output
}

pub(super) fn proto_scope(
    scope: Option<&wire::InstrumentationScope>,
    schema_url: &str,
) -> Option<InstrumentationScope> {
    let scope = scope?;
    Some(InstrumentationScope {
        name: if scope.name.is_empty() {
            "unknown".into()
        } else {
            scope.name.clone()
        },
        version: (!scope.version.is_empty()).then(|| scope.version.clone()),
        attributes: proto_attributes(&scope.attributes),
        schema_url: (!schema_url.is_empty()).then(|| schema_url.to_string()),
    })
}

pub(super) fn proto_attributes(values: &[wire::KeyValue]) -> BTreeMap<String, AttributeValue> {
    values
        .iter()
        .filter_map(|entry| {
            Some((
                entry.key.clone(),
                json_to_attribute(proto_any_json(entry.value.as_ref()?)),
            ))
        })
        .collect()
}

pub(super) fn proto_any_json(value: &AnyValue) -> Value {
    match value.value.as_ref() {
        Some(any_value::Value::StringValue(value)) => json!(value),
        Some(any_value::Value::BoolValue(value)) => json!(value),
        Some(any_value::Value::IntValue(value)) => json!(value),
        Some(any_value::Value::DoubleValue(value)) => json!(value),
        Some(any_value::Value::BytesValue(value)) => json!({"bytesBase64": BASE64.encode(value)}),
        Some(any_value::Value::StringValueStrindex(value)) => {
            json!({"stringValueStrindex": value})
        }
        Some(any_value::Value::ArrayValue(value)) => {
            Value::Array(value.values.iter().map(proto_any_json).collect())
        }
        Some(any_value::Value::KvlistValue(value)) => Value::Object(
            value
                .values
                .iter()
                .filter_map(|entry| {
                    Some((entry.key.clone(), proto_any_json(entry.value.as_ref()?)))
                })
                .collect(),
        ),
        None => Value::Null,
    }
}

pub(super) fn valid_proto_id(bytes: &[u8], expected: usize) -> Option<String> {
    (bytes.len() == expected && bytes.iter().any(|byte| *byte != 0)).then(|| hex::encode(bytes))
}
