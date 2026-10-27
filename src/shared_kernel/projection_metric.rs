//! Published metric read data. Field order and serialization are stable.

use crate::shared_kernel::event::{AttributeValue, MetricExemplar, MetricTemporality};
use crate::shared_kernel::query_limit::default_query_limit;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROJECTION_METRIC_STORE: &str = "metric-store";
pub const MAX_METRIC_QUERY_LIMIT: usize = 1_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistogramKind {
    Explicit,
    Exponential,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MetricChunkV1 {
    pub start_time_unix_nano: i64,
    pub end_time_unix_nano: i64,
    pub points: Vec<MetricPointV1>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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
    pub resource_equals: BTreeMap<String, String>,
    #[serde(default)]
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MetricSeriesResultV1 {
    pub series_id: String,
    pub project: String,
    pub environment: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub temporality: MetricTemporality,
    pub resource: BTreeMap<String, String>,
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MetricPage {
    pub series: Vec<MetricSeriesResultV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_series_id: Option<String>,
    pub projection_cursor: u64,
    pub has_more: bool,
    pub overflowed_series: u64,
    pub overflowed_points: u64,
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
}

fn is_false(value: &bool) -> bool {
    !*value
}
