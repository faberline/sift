//! Reading a series' points for a query: the point and byte counts checked from
//! chunk metadata before any chunk is loaded, and sealed chunks read back from
//! the store.

use anyhow::{bail, Context, Result};

use crate::projection::domain::metric::{MetricChunkV1, MetricPointV1};
use crate::projection::domain::metric_series::{
    point_is_in_range, point_order, time_ranges_overlap, SealedMetricChunk, StoredMetricSeries,
};
use crate::projection::infrastructure::metric_projection::MetricProjection;

#[cfg(test)]
use crate::projection::domain::metric_memory_size::METRIC_RESIDENT_POINT_MATERIALIZATIONS;

impl MetricProjection {
    pub(super) fn all_points(&self, series: &StoredMetricSeries) -> Result<Vec<MetricPointV1>> {
        let mut points = Vec::with_capacity(series.point_count);
        for sealed in &series.sealed_chunks {
            points.extend(self.read_sealed_chunk(sealed)?.points);
        }
        points.extend(
            series
                .chunks
                .iter()
                .flat_map(|chunk| chunk.points.iter().cloned()),
        );
        Ok(points)
    }

    pub(super) fn scan_point_count(
        &self,
        series: &StoredMetricSeries,
        start: Option<i64>,
        end: Option<i64>,
    ) -> Result<usize> {
        series
            .sealed_chunks
            .iter()
            .filter(|chunk| {
                time_ranges_overlap(
                    chunk.start_time_unix_nano,
                    chunk.end_time_unix_nano,
                    start,
                    end,
                )
            })
            .map(|chunk| {
                if chunk.point_count == 0 {
                    bail!("sealed metric chunk {} has no point metadata", chunk.key);
                }
                Ok(chunk.point_count)
            })
            .chain(
                series
                    .chunks
                    .iter()
                    .filter(|chunk| {
                        time_ranges_overlap(
                            chunk.start_time_unix_nano,
                            chunk.end_time_unix_nano,
                            start,
                            end,
                        )
                    })
                    .map(|chunk| {
                        if chunk.point_count == 0 {
                            bail!("resident metric chunk has no trusted point metadata");
                        }
                        Ok(chunk.point_count)
                    }),
            )
            .try_fold(0_usize, |total, count| {
                total
                    .checked_add(count?)
                    .context("metric query source point budget overflowed")
            })
    }

    pub(super) fn scan_materialized_bytes(
        &self,
        series: &StoredMetricSeries,
        start: Option<i64>,
        end: Option<i64>,
    ) -> Result<usize> {
        let sealed_bytes = series
            .sealed_chunks
            .iter()
            .filter(|chunk| {
                time_ranges_overlap(
                    chunk.start_time_unix_nano,
                    chunk.end_time_unix_nano,
                    start,
                    end,
                )
            })
            .try_fold(0_usize, |total, chunk| {
                if chunk.encoded_bytes == 0 || chunk.materialized_bytes == 0 {
                    bail!("sealed metric chunk {} has no byte metadata", chunk.key);
                }
                total
                    .checked_add(chunk.materialized_bytes.max(chunk.encoded_bytes))
                    .context("metric query source byte budget overflowed")
            })?;
        series
            .chunks
            .iter()
            .filter(|chunk| {
                time_ranges_overlap(
                    chunk.start_time_unix_nano,
                    chunk.end_time_unix_nano,
                    start,
                    end,
                )
            })
            .try_fold(sealed_bytes, |total, chunk| {
                if chunk.point_count == 0 || chunk.materialized_bytes == 0 {
                    bail!("resident metric chunk has no trusted size metadata");
                }
                total
                    .checked_add(chunk.materialized_bytes)
                    .context("metric query source byte budget overflowed")
            })
    }

    pub(super) fn points_for_scan(
        &self,
        series: &StoredMetricSeries,
        start: Option<i64>,
        end: Option<i64>,
    ) -> Result<Vec<MetricPointV1>> {
        let mut points = Vec::new();
        for sealed in &series.sealed_chunks {
            if !time_ranges_overlap(
                sealed.start_time_unix_nano,
                sealed.end_time_unix_nano,
                start,
                end,
            ) {
                continue;
            }
            points.extend(
                self.read_sealed_chunk(sealed)?
                    .points
                    .into_iter()
                    .filter(|point| point_is_in_range(point, start, end)),
            );
        }
        for chunk in &series.chunks {
            if !time_ranges_overlap(
                chunk.start_time_unix_nano,
                chunk.end_time_unix_nano,
                start,
                end,
            ) {
                continue;
            }
            #[cfg(test)]
            METRIC_RESIDENT_POINT_MATERIALIZATIONS.with(|materializations| {
                materializations.set(
                    materializations
                        .get()
                        .saturating_add(chunk.point_count as u64),
                );
            });
            points.extend(
                chunk
                    .points
                    .iter()
                    .filter(|point| point_is_in_range(point, start, end))
                    .cloned(),
            );
        }
        points.sort_by(point_order);
        Ok(points)
    }

    pub(super) fn last_point(&self, series: &StoredMetricSeries) -> Result<Option<MetricPointV1>> {
        if let Some(point) = series.chunks.last().and_then(|chunk| chunk.points.last()) {
            return Ok(Some(point.clone()));
        }
        let Some(sealed) = series.sealed_chunks.last() else {
            return Ok(None);
        };
        Ok(self.read_sealed_chunk(sealed)?.points.last().cloned())
    }

    pub(super) fn read_sealed_chunk(&self, sealed: &SealedMetricChunk) -> Result<MetricChunkV1> {
        self.chunk_store
            .as_ref()
            .context("metric projection has no disk chunk store")?
            .read(sealed)
    }
}
