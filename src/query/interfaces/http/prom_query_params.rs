//! The Prometheus-compatible query parameters, and the time and duration forms
//! they are written in.

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;

use crate::query::domain::promql::parse_decimal_seconds_nanos;

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

pub fn parse_prom_time_nanos(value: &str) -> Result<i64> {
    if let Ok(value) = DateTime::parse_from_rfc3339(value) {
        return value
            .timestamp_nanos_opt()
            .context("Prometheus timestamp is outside the supported range");
    }
    parse_decimal_seconds_nanos(value)
        .with_context(|| format!("invalid Prometheus timestamp `{value}`"))
}

pub fn parse_prom_duration_nanos(value: &str) -> Result<i64> {
    parse_decimal_seconds_nanos(value)
        .with_context(|| format!("invalid Prometheus duration `{value}`"))
}

pub fn nanos_rfc3339(nanos: i64) -> Result<String> {
    let seconds = nanos.div_euclid(1_000_000_000);
    let subsecond = nanos.rem_euclid(1_000_000_000) as u32;
    Ok(DateTime::<Utc>::from_timestamp(seconds, subsecond)
        .context("Prometheus timestamp is outside the supported range")?
        .to_rfc3339_opts(SecondsFormat::Nanos, true))
}

#[cfg(test)]
mod tests {
    use super::{nanos_rfc3339, parse_prom_time_nanos};

    #[test]
    fn prometheus_times_keep_exact_nanoseconds() {
        assert_eq!(
            parse_prom_time_nanos("1783987200.000999600").unwrap(),
            1_783_987_200_000_999_600
        );
        assert_eq!(
            parse_prom_time_nanos("2026-07-14T00:00:00.000999600Z").unwrap(),
            1_783_987_200_000_999_600
        );
        assert_eq!(
            nanos_rfc3339(1_783_987_200_000_999_600).unwrap(),
            "2026-07-14T00:00:00.000999600Z"
        );
    }
}
