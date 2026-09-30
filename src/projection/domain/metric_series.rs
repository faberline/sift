//! A metric series as the projection keeps it, with its resident and sealed
//! chunks, the state across series, and the series rules: identity, point
//! order, matching a query, and pushing and evicting points.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::projection::domain::metric::{
    MetricChunkV1, MetricPointV1, MetricQuery, MetricRollupV1, ResidentMetricChunk,
    METRIC_CHUNK_POINTS,
};
use crate::projection::domain::metric_memory_size::{
    metric_chunk_memory_bytes, metric_point_memory_bytes, MetricMemoryError,
};
use crate::{AttributeValue, MetricPoint, MetricTemporality};

#[derive(Clone, Deserialize, Serialize)]
pub(in crate::projection) struct StoredMetricSeries {
    pub(in crate::projection) series_id: String,
    pub(in crate::projection) project: String,
    pub(in crate::projection) environment: String,
    pub(in crate::projection) name: String,
    pub(in crate::projection) unit: Option<String>,
    pub(in crate::projection) temporality: MetricTemporality,
    pub(in crate::projection) resource: BTreeMap<String, String>,
    pub(in crate::projection) attributes: BTreeMap<String, AttributeValue>,
    pub(in crate::projection) overflow: bool,
    #[serde(default)]
    pub(in crate::projection) sealed_chunks: Vec<SealedMetricChunk>,
    pub(in crate::projection) chunks: Vec<ResidentMetricChunk>,
    #[serde(default)]
    pub(in crate::projection) point_count: usize,
    pub(in crate::projection) rollups: Vec<MetricRollupV1>,
    #[serde(skip)]
    pub(in crate::projection) resident_bytes: usize,
}

#[derive(Clone, Deserialize, Serialize)]
pub(in crate::projection) struct SealedMetricChunk {
    pub(in crate::projection) key: String,
    pub(in crate::projection) start_time_unix_nano: i64,
    pub(in crate::projection) end_time_unix_nano: i64,
    pub(in crate::projection) point_count: usize,
    pub(in crate::projection) encoded_bytes: usize,
    pub(in crate::projection) materialized_bytes: usize,
    pub(in crate::projection) sha256: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub(in crate::projection) struct MetricState {
    pub(in crate::projection) series: BTreeMap<String, StoredMetricSeries>,
    pub(in crate::projection) exact_identities: BTreeMap<String, BTreeSet<String>>,
    pub(in crate::projection) overflowed_identities: BTreeSet<String>,
    pub(in crate::projection) overflowed_points: u64,
    #[serde(default)]
    pub(in crate::projection) projection_cursor: u64,
    #[serde(skip)]
    pub(in crate::projection) memtable_bytes: usize,
}

pub(in crate::projection) fn metric_series_matches(
    series: &StoredMetricSeries,
    query: &MetricQuery,
) -> bool {
    series.project == query.project
        && query
            .environment
            .as_ref()
            .is_none_or(|environment| &series.environment == environment)
        && query.name.as_ref().is_none_or(|name| &series.name == name)
        && query
            .resource_equals
            .iter()
            .all(|(key, value)| series.resource.get(key) == Some(value))
        && query
            .attribute_equals
            .iter()
            .all(|(key, value)| series.attributes.get(key) == Some(value))
}

pub(in crate::projection) fn time_ranges_overlap(
    chunk_start: i64,
    chunk_end: i64,
    query_start: Option<i64>,
    query_end: Option<i64>,
) -> bool {
    query_start.is_none_or(|start| chunk_end >= start)
        && query_end.is_none_or(|end| chunk_start < end)
}

pub(in crate::projection) fn point_is_in_range(
    point: &MetricPointV1,
    start: Option<i64>,
    end: Option<i64>,
) -> bool {
    start.is_none_or(|start| point.time_unix_nano >= start)
        && end.is_none_or(|end| point.time_unix_nano < end)
}

#[derive(Debug, Error)]
pub(in crate::projection) enum MetricSeriesError {
    #[error("metric series id is not a SHA-256 hex digest")]
    InvalidSeriesId,
    #[error("resident metric chunk point count overflowed")]
    ChunkPointCountOverflow,
    #[error("resident metric chunk byte count overflowed")]
    ChunkByteCountOverflow,
    #[error("metric series point count overflowed")]
    SeriesPointCountOverflow,
    #[error("metric series resident byte count overflowed")]
    SeriesByteCountOverflow,
    #[error("resident metric chunk point count underflowed")]
    ChunkPointCountUnderflow,
    #[error("resident metric chunk byte count underflowed")]
    ChunkByteCountUnderflow,
    #[error("metric series resident byte count underflowed")]
    SeriesByteCountUnderflow,
    #[error("cannot build an empty metric chunk")]
    EmptyChunkBuild,
    #[error("metric chunk lost its tail")]
    ChunkLostTail,
    #[error("resident metric chunk cannot be empty")]
    EmptyResidentChunk,
    #[error("resident metric chunk byte metadata must be non-zero")]
    ZeroChunkBytes,
    #[error(transparent)]
    Identity(#[from] serde_json::Error),
    #[error(transparent)]
    MemorySize(#[from] MetricMemoryError),
}

pub(in crate::projection) fn validate_series_id(series_id: &str) -> Result<(), MetricSeriesError> {
    if series_id.len() != 64 || !series_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(MetricSeriesError::InvalidSeriesId);
    }
    Ok(())
}

pub(in crate::projection) fn identity_material(
    project: &str,
    environment: &str,
    metric: &MetricPoint,
    resource: &BTreeMap<String, String>,
    attributes: &BTreeMap<String, AttributeValue>,
    overflow: bool,
) -> Result<String, MetricSeriesError> {
    serde_json::to_string(&serde_json::json!({
        "project": project,
        "environment": environment,
        "name": metric.name,
        "unit": metric.unit,
        "temporality": metric.temporality,
        "resource": resource,
        "attributes": attributes,
        "overflow": overflow,
    }))
    .map_err(Into::into)
}

pub(in crate::projection) fn point_order(
    left: &MetricPointV1,
    right: &MetricPointV1,
) -> std::cmp::Ordering {
    left.time_unix_nano
        .cmp(&right.time_unix_nano)
        .then_with(|| left.cursor.cmp(&right.cursor))
        .then_with(|| left.event_id.cmp(&right.event_id))
}

pub(in crate::projection) fn push_metric_point(
    series: &mut StoredMetricSeries,
    point: MetricPointV1,
) -> Result<(), MetricSeriesError> {
    let resident_bytes = metric_point_memory_bytes(&point)?;
    let needs_chunk = series
        .chunks
        .last()
        .is_none_or(|chunk| chunk.points.len() == METRIC_CHUNK_POINTS);
    if needs_chunk {
        series.chunks.push(ResidentMetricChunk {
            chunk: MetricChunkV1 {
                start_time_unix_nano: point.time_unix_nano,
                end_time_unix_nano: point.time_unix_nano,
                points: Vec::with_capacity(METRIC_CHUNK_POINTS),
            },
            point_count: 0,
            materialized_bytes: 0,
        });
    }
    let chunk = series.chunks.last_mut().expect("metric chunk was created");
    if chunk.points.is_empty() {
        chunk.start_time_unix_nano = point.time_unix_nano;
    }
    chunk.end_time_unix_nano = point.time_unix_nano;
    chunk.points.push(point);
    chunk.point_count = chunk
        .point_count
        .checked_add(1)
        .ok_or(MetricSeriesError::ChunkPointCountOverflow)?;
    chunk.materialized_bytes = chunk
        .materialized_bytes
        .checked_add(resident_bytes)
        .ok_or(MetricSeriesError::ChunkByteCountOverflow)?;
    series.point_count = series
        .point_count
        .checked_add(1)
        .ok_or(MetricSeriesError::SeriesPointCountOverflow)?;
    series.resident_bytes = series
        .resident_bytes
        .checked_add(resident_bytes)
        .ok_or(MetricSeriesError::SeriesByteCountOverflow)?;
    Ok(())
}

pub(in crate::projection) fn pop_oldest_metric_point(
    series: &mut StoredMetricSeries,
) -> Result<Option<String>, MetricSeriesError> {
    let Some(chunk) = series.chunks.first_mut() else {
        return Ok(None);
    };
    let point = chunk.points.remove(0);
    let resident_bytes = metric_point_memory_bytes(&point)?;
    series.point_count = series.point_count.saturating_sub(1);
    chunk.point_count = chunk
        .point_count
        .checked_sub(1)
        .ok_or(MetricSeriesError::ChunkPointCountUnderflow)?;
    chunk.materialized_bytes = chunk
        .materialized_bytes
        .checked_sub(resident_bytes)
        .ok_or(MetricSeriesError::ChunkByteCountUnderflow)?;
    if series.chunks[0].points.is_empty() {
        series.chunks.remove(0);
    } else {
        series.chunks[0].start_time_unix_nano = series.chunks[0].points[0].time_unix_nano;
    }
    series.resident_bytes = series
        .resident_bytes
        .checked_sub(resident_bytes)
        .ok_or(MetricSeriesError::SeriesByteCountUnderflow)?;
    Ok(Some(point.event_id))
}

pub(in crate::projection) fn make_chunks(
    points: &[MetricPointV1],
) -> Result<Vec<ResidentMetricChunk>, MetricSeriesError> {
    points
        .chunks(METRIC_CHUNK_POINTS)
        .map(|points| {
            let first = points.first().ok_or(MetricSeriesError::EmptyChunkBuild)?;
            let last = points.last().ok_or(MetricSeriesError::ChunkLostTail)?;
            resident_metric_chunk(MetricChunkV1 {
                start_time_unix_nano: first.time_unix_nano,
                end_time_unix_nano: last.time_unix_nano,
                points: points.to_vec(),
            })
        })
        .collect()
}

pub(in crate::projection) fn resident_metric_chunk(
    chunk: MetricChunkV1,
) -> Result<ResidentMetricChunk, MetricSeriesError> {
    if chunk.points.is_empty() {
        return Err(MetricSeriesError::EmptyResidentChunk);
    }
    let point_count = chunk.points.len();
    let materialized_bytes = metric_chunk_memory_bytes(&chunk)?;
    if materialized_bytes == 0 {
        return Err(MetricSeriesError::ZeroChunkBytes);
    }
    Ok(ResidentMetricChunk {
        chunk,
        point_count,
        materialized_bytes,
    })
}

pub(in crate::projection) fn sha256(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(bytes.as_ref()))
}
