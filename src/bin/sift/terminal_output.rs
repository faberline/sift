//! Printing a command's result as pretty JSON, from a value or from JSON text,
//! with `next: done` added.

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;

pub(super) fn print_json_text_terminal(text: &str) -> Result<()> {
    let value: Value = serde_json::from_str(text).context("parse machine-readable CLI output")?;
    print_json_terminal(value)
}

pub(super) fn print_json_terminal(value: impl Serialize) -> Result<()> {
    let value = serde_json::to_value(value)?;
    let output = match value {
        Value::Object(mut object) => {
            object.insert("next".to_string(), Value::String("done".to_string()));
            Value::Object(object)
        }
        value => serde_json::json!({ "result": value, "next": "done" }),
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
