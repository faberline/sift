//! Metric points, chunks, histograms and rollups as the metric projection keeps
//! and returns them, the limits it runs under, and the query and page over
//! series.

use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::projection::domain::metric_aggregation::{merge_counts, optional_max, optional_min};
use crate::{AttributeValue, MetricExemplar, MetricTemporality};

pub const PROJECTION_METRIC_STORE: &str = "metric-store";
pub const METRIC_SCHEMA_VERSION: u32 = 7;
pub const METRIC_CHUNK_POINTS: usize = 256;
pub const DEFAULT_METRIC_MEMTABLE_BYTES: usize = 256 * 1024 * 1024;
pub const DEFAULT_METRIC_CARDINALITY_LIMIT: usize = 10_000;
pub const DEFAULT_RETAINED_POINTS_PER_SERIES: usize = 100_000;
pub const MAX_METRIC_QUERY_LIMIT: usize = 1_000;
pub const MAX_METRIC_QUERY_SOURCE_POINTS: usize = 250_000;
pub const MAX_METRIC_QUERY_SOURCE_BYTES: usize = 64 * 1024 * 1024;
pub const ROLLUP_WINDOWS_SECONDS: [u64; 2] = [60, 3_600];
pub(in crate::projection) const MAX_ROLLUP_LOOKBACK_NANOS: i64 = 3_600 * 1_000_000_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HistogramKind {
    Explicit,
    Exponential,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct MetricHistogramV1 {
    pub kind: HistogramKind,
    pub count: u64,
    pub sum: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub explicit_bounds: Vec<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bucket_counts: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<i32>,
    #[serde(default)]
    pub zero_count: u64,
    #[serde(default)]
    pub positive_offset: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub positive_bucket_counts: Vec<u64>,
    #[serde(default)]
    pub negative_offset: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub negative_bucket_counts: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

impl MetricHistogramV1 {
    pub(in crate::projection) fn validate(&self) -> Result<()> {
        if !self.sum.is_finite()
            || self.min.is_some_and(|value| !value.is_finite())
            || self.max.is_some_and(|value| !value.is_finite())
            || self.explicit_bounds.iter().any(|value| !value.is_finite())
        {
            bail!("histogram numbers must be finite");
        }
        if self.min.zip(self.max).is_some_and(|(min, max)| min > max) {
            bail!("histogram min must not exceed max");
        }
        match self.kind {
            HistogramKind::Explicit => {
                if self.scale.is_some()
                    || !self.positive_bucket_counts.is_empty()
                    || !self.negative_bucket_counts.is_empty()
                {
                    bail!("explicit histogram cannot contain exponential buckets");
                }
                if self.bucket_counts.len() != self.explicit_bounds.len() + 1 {
                    bail!("explicit histogram bucket_counts must have bounds length plus one");
                }
                if !self
                    .explicit_bounds
                    .windows(2)
                    .all(|window| window[0] < window[1])
                {
                    bail!("explicit histogram bounds must be strictly increasing");
                }
                if self.bucket_counts.iter().sum::<u64>() != self.count {
                    bail!("explicit histogram buckets must sum to count");
                }
            }
            HistogramKind::Exponential => {
                if self.scale.is_none()
                    || !self.explicit_bounds.is_empty()
                    || !self.bucket_counts.is_empty()
                {
                    bail!("exponential histogram requires scale and no explicit buckets");
                }
                let total = self.zero_count
                    + self.positive_bucket_counts.iter().sum::<u64>()
                    + self.negative_bucket_counts.iter().sum::<u64>();
                if total != self.count {
                    bail!("exponential histogram buckets must sum to count");
                }
            }
        }
        Ok(())
    }

    pub(super) fn merge(&mut self, other: &Self) -> Result<()> {
        self.validate()?;
        other.validate()?;
        if self.kind != other.kind
            || self.explicit_bounds != other.explicit_bounds
            || self.scale != other.scale
            || self.positive_offset != other.positive_offset
            || self.negative_offset != other.negative_offset
            || self.bucket_counts.len() != other.bucket_counts.len()
            || self.positive_bucket_counts.len() != other.positive_bucket_counts.len()
            || self.negative_bucket_counts.len() != other.negative_bucket_counts.len()
        {
            bail!("histogram schemas are not merge compatible");
        }
        self.count = self.count.saturating_add(other.count);
        self.sum += other.sum;
        self.zero_count = self.zero_count.saturating_add(other.zero_count);
        merge_counts(&mut self.bucket_counts, &other.bucket_counts);
        merge_counts(
            &mut self.positive_bucket_counts,
            &other.positive_bucket_counts,
        );
        merge_counts(
            &mut self.negative_bucket_counts,
            &other.negative_bucket_counts,
        );
        self.min = optional_min(self.min, other.min);
        self.max = optional_max(self.max, other.max);
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct MetricPointV1 {
    pub cursor: u64,
    pub event_id: String,
    pub occurred_at: String,
    pub time_unix_nano: i64,
    pub value: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub histogram: Option<MetricHistogramV1>,
    #[serde(default)]
    pub exemplars: Vec<MetricExemplar>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct MetricChunkV1 {
    pub start_time_unix_nano: i64,
    pub end_time_unix_nano: i64,
    pub points: Vec<MetricPointV1>,
}

#[derive(Clone, Deserialize, Serialize)]
pub(in crate::projection) struct ResidentMetricChunk {
    pub(in crate::projection) chunk: MetricChunkV1,
    pub(in crate::projection) point_count: usize,
    pub(in crate::projection) materialized_bytes: usize,
}

impl Deref for ResidentMetricChunk {
    type Target = MetricChunkV1;

    fn deref(&self) -> &Self::Target {
        &self.chunk
    }
}

impl DerefMut for ResidentMetricChunk {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.chunk
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct MetricRollupV1 {
    pub window_seconds: u64,
    pub start_time_unix_nano: i64,
    pub end_time_unix_nano: i64,
    pub point_count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub last: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub histogram: Option<MetricHistogramV1>,
    #[serde(default)]
    pub exemplars: Vec<MetricExemplar>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetricAggregation {
    #[default]
    Raw,
    Sum,
    Avg,
    Min,
    Max,
    Count,
    Rate,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct MetricQuery {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub resource_equals: BTreeMap<String, String>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub attribute_equals: BTreeMap<String, AttributeValue>,
    #[serde(default)]
    pub aggregation: MetricAggregation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_series_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_cursor: Option<u64>,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
}

impl MetricQuery {
    pub fn for_project(project: impl Into<String>) -> Self {
        Self {
            project: project.into(),
            environment: None,
            name: None,
            start_time: None,
            end_time: None,
            resource_equals: BTreeMap::new(),
            attribute_equals: BTreeMap::new(),
            aggregation: MetricAggregation::Raw,
            after_series_id: None,
            min_cursor: None,
            limit: default_query_limit(),
        }
    }

    pub(in crate::projection) fn validate(&self) -> Result<(Option<i64>, Option<i64>)> {
        if self.project.trim().is_empty() {
            bail!("project must not be empty");
        }
        if self.limit == 0 || self.limit > MAX_METRIC_QUERY_LIMIT {
            bail!("limit must be between 1 and {MAX_METRIC_QUERY_LIMIT}");
        }
        let start = parse_optional_time("start_time", self.start_time.as_deref())?;
        let end = parse_optional_time("end_time", self.end_time.as_deref())?;
        if start.zip(end).is_some_and(|(start, end)| start >= end) {
            bail!("start_time must be earlier than end_time");
        }
        Ok((start, end))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct MetricSeriesResultV1 {
    pub series_id: String,
    pub project: String,
    pub environment: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub temporality: MetricTemporality,
    pub resource: BTreeMap<String, String>,
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, AttributeValue>,
    pub overflow: bool,
    pub points: Vec<MetricPointV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub histogram: Option<MetricHistogramV1>,
    pub reset_count: u64,
    pub rollups: Vec<MetricRollupV1>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, ToSchema)]
pub struct MetricPage {
    pub series: Vec<MetricSeriesResultV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_series_id: Option<String>,
    pub projection_cursor: u64,
    pub has_more: bool,
    pub overflowed_series: u64,
    pub overflowed_points: u64,
}

fn parse_optional_time(name: &str, value: Option<&str>) -> Result<Option<i64>> {
    value
        .map(|value| parse_time(value).with_context(|| format!("{name} must be RFC3339")))
        .transpose()
}

pub(in crate::projection) fn parse_time(value: &str) -> Result<i64> {
    DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("invalid RFC3339 timestamp `{value}`"))?
        .with_timezone(&Utc)
        .timestamp_nanos_opt()
        .context("timestamp is outside nanosecond range")
}

const fn default_query_limit() -> usize {
    100
}

fn is_false(value: &bool) -> bool {
    !*value
}
