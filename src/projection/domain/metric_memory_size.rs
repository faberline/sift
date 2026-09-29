//! The resident memory a metric point, chunk or series is charged against the
//! memtable limit, and the counters tests read of how often it is measured.

use anyhow::{bail, Context, Result};

use crate::projection::domain::metric::{MetricChunkV1, MetricPointV1};
use crate::projection::domain::metric_series::StoredMetricSeries;
use crate::MetricExemplar;

#[cfg(test)]
thread_local! {
    pub(in crate::projection) static METRIC_POINT_MEMORY_MEASUREMENTS: std::cell::Cell<u64> = const {
        std::cell::Cell::new(0)
    };
    pub(in crate::projection) static METRIC_RESIDENT_POINT_MATERIALIZATIONS: std::cell::Cell<u64> = const {
        std::cell::Cell::new(0)
    };
}

pub(in crate::projection) fn metric_series_memory_bytes(
    series: &StoredMetricSeries,
) -> Result<usize> {
    series.chunks.iter().try_fold(0_usize, |total, chunk| {
        if chunk.point_count == 0 || chunk.materialized_bytes == 0 {
            bail!("resident metric chunk has no trusted size metadata");
        }
        total
            .checked_add(chunk.materialized_bytes)
            .context("metric resident byte accounting overflowed")
    })
}

pub(in crate::projection) fn metric_chunk_memory_bytes(chunk: &MetricChunkV1) -> Result<usize> {
    chunk.points.iter().try_fold(0_usize, |total, point| {
        Ok(total.saturating_add(metric_point_memory_bytes(point)?))
    })
}

pub(super) fn metric_point_memory_bytes(point: &MetricPointV1) -> Result<usize> {
    #[cfg(test)]
    METRIC_POINT_MEMORY_MEASUREMENTS.with(|measurements| {
        measurements.set(measurements.get().saturating_add(1));
    });
    let encoded_bytes = serde_json::to_vec(point)
        .context("measure metric memtable point")?
        .len();
    let exemplar_bytes = point.exemplars.iter().fold(0_usize, |total, exemplar| {
        total
            .saturating_add(std::mem::size_of::<MetricExemplar>())
            .saturating_add(exemplar.trace_id.len())
            .saturating_add(exemplar.span_id.len())
    });
    let histogram_bytes = point.histogram.as_ref().map_or(0, |histogram| {
        histogram
            .explicit_bounds
            .len()
            .saturating_mul(std::mem::size_of::<f64>())
            .saturating_add(
                histogram
                    .bucket_counts
                    .len()
                    .saturating_mul(std::mem::size_of::<u64>()),
            )
            .saturating_add(
                histogram
                    .positive_bucket_counts
                    .len()
                    .saturating_mul(std::mem::size_of::<u64>()),
            )
            .saturating_add(
                histogram
                    .negative_bucket_counts
                    .len()
                    .saturating_mul(std::mem::size_of::<u64>()),
            )
    });
    Ok(encoded_bytes
        .saturating_add(std::mem::size_of::<MetricPointV1>())
        .saturating_add(point.event_id.len())
        .saturating_add(point.occurred_at.len())
        .saturating_add(exemplar_bytes)
        .saturating_add(histogram_bytes))
}
