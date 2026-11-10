//! The versioned filter data shared by persisted jobs and query adapters.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
// Keep this published V1 Rust enum and its wire shape stable.
#[allow(clippy::large_enum_variant)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryExpressionV1 {
    And {
        args: Vec<QueryExpressionV1>,
    },
    Or {
        args: Vec<QueryExpressionV1>,
    },
    Not {
        arg: Box<QueryExpressionV1>,
    },
    Eq {
        field: String,
        value: serde_json::Value,
    },
    In {
        field: String,
        values: Vec<serde_json::Value>,
    },
    Exists {
        field: String,
    },
    Range {
        field: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gt: Option<Box<serde_json::Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gte: Option<Box<serde_json::Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lt: Option<Box<serde_json::Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lte: Option<Box<serde_json::Value>>,
    },
    Text {
        field: String,
        value: String,
    },
    Regex {
        field: String,
        pattern: String,
    },
}
