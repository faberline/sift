//! Pure Prometheus endpoint parameters.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct InstantQueryParams {
    pub project: String,
    #[serde(default)]
    pub environment: Option<String>,
    pub query: String,
    #[serde(default)]
    pub time: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RangeQueryParams {
    pub project: String,
    #[serde(default)]
    pub environment: Option<String>,
    pub query: String,
    pub start: String,
    pub end: String,
    pub step: String,
}
