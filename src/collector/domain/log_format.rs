//! Native log parsing options. Existing library callers keep strict parsing.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    #[default]
    Auto,
    Structured,
    Json,
    Text,
}

#[derive(Clone, Debug, Default)]
pub struct LogOptions {
    pub format: LogFormat,
    pub multiline_start: Option<String>,
}
