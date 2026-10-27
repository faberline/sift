//! Metric points, chunks, histograms and rollups as the metric projection keeps
//! and returns them, the limits it runs under, and the query and page over
//! series.

use std::ops::{Deref, DerefMut};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::projection::domain::metric_aggregation::{merge_counts, optional_max, optional_min};

pub const METRIC_SCHEMA_VERSION: u32 = 7;
pub const METRIC_CHUNK_POINTS: usize = 256;
pub const DEFAULT_METRIC_MEMTABLE_BYTES: usize = 256 * 1024 * 1024;
pub const DEFAULT_METRIC_CARDINALITY_LIMIT: usize = 10_000;
pub const DEFAULT_RETAINED_POINTS_PER_SERIES: usize = 100_000;
pub const MAX_METRIC_QUERY_SOURCE_POINTS: usize = 250_000;
pub const MAX_METRIC_QUERY_SOURCE_BYTES: usize = 64 * 1024 * 1024;
pub const ROLLUP_WINDOWS_SECONDS: [u64; 2] = [60, 3_600];
pub(in crate::projection) const MAX_ROLLUP_LOOKBACK_NANOS: i64 = 3_600 * 1_000_000_000;

#[derive(Debug, Error)]
pub(in crate::projection) enum MetricError {
    #[error("histogram numbers must be finite")]
    HistogramNotFinite,
    #[error("histogram min must not exceed max")]
    HistogramMinAboveMax,
    #[error("explicit histogram cannot contain exponential buckets")]
    ExplicitWithExponentialBuckets,
    #[error("explicit histogram bucket_counts must have bounds length plus one")]
    ExplicitBucketCount,
    #[error("explicit histogram bounds must be strictly increasing")]
    ExplicitBoundsOrder,
    #[error("explicit histogram buckets must sum to count")]
    ExplicitBucketSum,
    #[error("exponential histogram requires scale and no explicit buckets")]
    ExponentialShape,
    #[error("exponential histogram buckets must sum to count")]
    ExponentialBucketSum,
    #[error("histogram schemas are not merge compatible")]
    IncompatibleSchemas,
    #[error("project must not be empty")]
    EmptyProject,
    #[error("limit must be between 1 and {max}", max = MAX_METRIC_QUERY_LIMIT)]
    Limit,
    #[error("start_time must be earlier than end_time")]
    TimeOrder,
    #[error("{name} must be RFC3339")]
    TimeNotRfc3339 {
        name: &'static str,
        #[source]
        source: Box<MetricError>,
    },
    #[error("invalid RFC3339 timestamp `{value}`")]
    InvalidTimestamp {
        value: String,
        #[source]
        source: chrono::ParseError,
    },
    #[error("timestamp is outside nanosecond range")]
    TimestampOutOfRange,
}

impl MetricHistogramV1 {
    pub(in crate::projection) fn validate(&self) -> Result<(), MetricError> {
        if !self.sum.is_finite()
            || self.min.is_some_and(|value| !value.is_finite())
            || self.max.is_some_and(|value| !value.is_finite())
            || self.explicit_bounds.iter().any(|value| !value.is_finite())
        {
            return Err(MetricError::HistogramNotFinite);
        }
        if self.min.zip(self.max).is_some_and(|(min, max)| min > max) {
            return Err(MetricError::HistogramMinAboveMax);
        }
        match self.kind {
            HistogramKind::Explicit => {
                if self.scale.is_some()
                    || !self.positive_bucket_counts.is_empty()
                    || !self.negative_bucket_counts.is_empty()
                {
                    return Err(MetricError::ExplicitWithExponentialBuckets);
                }
                if self.bucket_counts.len() != self.explicit_bounds.len() + 1 {
                    return Err(MetricError::ExplicitBucketCount);
                }
                if !self
                    .explicit_bounds
                    .windows(2)
                    .all(|window| window[0] < window[1])
                {
                    return Err(MetricError::ExplicitBoundsOrder);
                }
                if self.bucket_counts.iter().sum::<u64>() != self.count {
                    return Err(MetricError::ExplicitBucketSum);
                }
            }
            HistogramKind::Exponential => {
                if self.scale.is_none()
                    || !self.explicit_bounds.is_empty()
                    || !self.bucket_counts.is_empty()
                {
                    return Err(MetricError::ExponentialShape);
                }
                let total = self.zero_count
                    + self.positive_bucket_counts.iter().sum::<u64>()
                    + self.negative_bucket_counts.iter().sum::<u64>();
                if total != self.count {
                    return Err(MetricError::ExponentialBucketSum);
                }
            }
        }
        Ok(())
    }

    pub(super) fn merge(&mut self, other: &Self) -> Result<(), MetricError> {
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
            return Err(MetricError::IncompatibleSchemas);
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

impl MetricQuery {
    pub(in crate::projection) fn validate(
        &self,
    ) -> Result<(Option<i64>, Option<i64>), MetricError> {
        if self.project.trim().is_empty() {
            return Err(MetricError::EmptyProject);
        }
        if self.limit == 0 || self.limit > MAX_METRIC_QUERY_LIMIT {
            return Err(MetricError::Limit);
        }
        let start = parse_optional_time("start_time", self.start_time.as_deref())?;
        let end = parse_optional_time("end_time", self.end_time.as_deref())?;
        if start.zip(end).is_some_and(|(start, end)| start >= end) {
            return Err(MetricError::TimeOrder);
        }
        Ok((start, end))
    }
}

fn parse_optional_time(
    name: &'static str,
    value: Option<&str>,
) -> Result<Option<i64>, MetricError> {
    value
        .map(|value| {
            parse_time(value).map_err(|source| MetricError::TimeNotRfc3339 {
                name,
                source: Box::new(source),
            })
        })
        .transpose()
}

pub(in crate::projection) fn parse_time(value: &str) -> Result<i64, MetricError> {
    DateTime::parse_from_rfc3339(value)
        .map_err(|source| MetricError::InvalidTimestamp {
            value: value.to_owned(),
            source,
        })?
        .with_timezone(&Utc)
        .timestamp_nanos_opt()
        .ok_or(MetricError::TimestampOutOfRange)
}

pub use crate::shared_kernel::projection_metric::{
    HistogramKind, MetricAggregation, MetricChunkV1, MetricHistogramV1, MetricPage, MetricPointV1,
    MetricQuery, MetricRollupV1, MetricSeriesResultV1, MAX_METRIC_QUERY_LIMIT,
    PROJECTION_METRIC_STORE,
};
