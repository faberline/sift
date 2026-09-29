//! Fixtures for the metric projection's tests: points, series and chunks, and
//! the thread-local counters of point measurements and materializations.

mod chunk_metadata;
mod query_budget;

use std::collections::BTreeMap;

use crate::projection::domain::metric::{MetricChunkV1, MetricPointV1};
use crate::projection::domain::metric_memory_size::{
    METRIC_POINT_MEMORY_MEASUREMENTS, METRIC_RESIDENT_POINT_MATERIALIZATIONS,
};
use crate::projection::domain::metric_series::{
    resident_metric_chunk, SealedMetricChunk, StoredMetricSeries,
};
use crate::MetricTemporality;

use super::*;

fn test_point(cursor: u64, time_unix_nano: i64) -> MetricPointV1 {
    MetricPointV1 {
        cursor,
        event_id: format!("event-{cursor}"),
        occurred_at: "1970-01-01T00:00:10Z".into(),
        time_unix_nano,
        value: cursor as f64,
        stale: false,
        histogram: None,
        exemplars: Vec::new(),
    }
}

fn test_series(
    series_id: &str,
    sealed_chunks: Vec<SealedMetricChunk>,
    chunks: Vec<MetricChunkV1>,
) -> StoredMetricSeries {
    let chunks = chunks
        .into_iter()
        .map(|chunk| resident_metric_chunk(chunk).unwrap())
        .collect::<Vec<_>>();
    let point_count = sealed_chunks
        .iter()
        .map(|chunk| chunk.point_count)
        .chain(chunks.iter().map(|chunk| chunk.point_count))
        .sum();
    let resident_bytes = chunks.iter().map(|chunk| chunk.materialized_bytes).sum();
    StoredMetricSeries {
        series_id: series_id.into(),
        project: "project-a".into(),
        environment: "prod".into(),
        name: "bounded_metric".into(),
        unit: None,
        temporality: MetricTemporality::Gauge,
        resource: BTreeMap::new(),
        attributes: BTreeMap::new(),
        overflow: false,
        sealed_chunks,
        chunks,
        point_count,
        rollups: Vec::new(),
        resident_bytes,
    }
}

fn chunk(points: Vec<MetricPointV1>) -> MetricChunkV1 {
    MetricChunkV1 {
        start_time_unix_nano: points.first().unwrap().time_unix_nano,
        end_time_unix_nano: points.last().unwrap().time_unix_nano,
        points,
    }
}

fn reset_point_memory_measurements() {
    METRIC_POINT_MEMORY_MEASUREMENTS.with(|measurements| measurements.set(0));
    METRIC_RESIDENT_POINT_MATERIALIZATIONS.with(|materializations| materializations.set(0));
}

fn point_memory_measurements() -> u64 {
    METRIC_POINT_MEMORY_MEASUREMENTS.with(std::cell::Cell::get)
}

fn resident_point_materializations() -> u64 {
    METRIC_RESIDENT_POINT_MATERIALIZATIONS.with(std::cell::Cell::get)
}
