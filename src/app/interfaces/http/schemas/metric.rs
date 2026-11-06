//! OpenAPI descriptions for metric data. Runtime bytes use the original models.

use crate::{AttributeValue, MetricExemplar, MetricTemporality};
use std::collections::BTreeMap;
fn is_false(value: &bool) -> bool {
    !*value
}

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HistogramKind {
    Explicit,
    Exponential,
}

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
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

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
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

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct MetricChunkV1 {
    pub start_time_unix_nano: i64,
    pub end_time_unix_nano: i64,
    pub points: Vec<MetricPointV1>,
}

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
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

#[allow(dead_code)]
#[derive(Default, serde::Serialize, utoipa::ToSchema)]
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

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
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

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
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

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct MetricPage {
    pub series: Vec<MetricSeriesResultV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_series_id: Option<String>,
    pub projection_cursor: u64,
    pub has_more: bool,
    pub overflowed_series: u64,
    pub overflowed_points: u64,
}

schema_adapter!(
    crate::projection::domain::metric::HistogramKind,
    HistogramKind
);
schema_adapter!(
    crate::projection::domain::metric::MetricHistogramV1,
    MetricHistogramV1
);
schema_adapter!(
    crate::projection::domain::metric::MetricPointV1,
    MetricPointV1
);
schema_adapter!(
    crate::projection::domain::metric::MetricChunkV1,
    MetricChunkV1
);
schema_adapter!(
    crate::projection::domain::metric::MetricRollupV1,
    MetricRollupV1
);
schema_adapter!(
    crate::projection::domain::metric::MetricAggregation,
    MetricAggregation
);
schema_adapter!(crate::projection::domain::metric::MetricQuery, MetricQuery);
schema_adapter!(
    crate::projection::domain::metric::MetricSeriesResultV1,
    MetricSeriesResultV1
);
schema_adapter!(crate::projection::domain::metric::MetricPage, MetricPage);
