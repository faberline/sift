//! Sealing full resident chunks to the chunk store, evicting a series' oldest
//! points past its retention, and keeping the memtable under its byte limit.

use anyhow::{bail, Context, Result};

use crate::projection::domain::metric::METRIC_CHUNK_POINTS;
use crate::projection::domain::metric_series::{
    pop_oldest_metric_point, MetricState, StoredMetricSeries,
};
use crate::projection::infrastructure::metric_projection::MetricProjection;

impl MetricProjection {
    pub(super) fn seal_full_chunks(&self, series: &mut StoredMetricSeries) -> Result<()> {
        let Some(store) = &self.chunk_store else {
            return Ok(());
        };
        while series
            .chunks
            .first()
            .is_some_and(|chunk| chunk.points.len() >= METRIC_CHUNK_POINTS)
        {
            let resident_bytes = series.chunks[0].materialized_bytes;
            let sealed = store.write(&series.series_id, &series.chunks[0])?;
            series.chunks.remove(0);
            series.resident_bytes = series.resident_bytes.saturating_sub(resident_bytes);
            series.sealed_chunks.push(sealed);
        }
        Ok(())
    }

    pub(super) fn evict_series_to_limit(
        &self,
        series: &mut StoredMetricSeries,
        mutation_cursor: u64,
    ) -> Result<u64> {
        let mut work = 0_u64;
        while series.point_count > self.retained_points_per_series {
            if let Some(sealed) = series.sealed_chunks.first().cloned() {
                let chunk = self.read_sealed_chunk(&sealed)?;
                if let Some(store) = &self.chunk_store {
                    store.mark_obsolete(mutation_cursor, std::slice::from_ref(&sealed.key))?;
                }
                series.sealed_chunks.remove(0);
                series.point_count = series.point_count.saturating_sub(chunk.points.len());
                work = work.saturating_add(chunk.points.len() as u64);
                continue;
            }
            let Some(_) = pop_oldest_metric_point(series)? else {
                break;
            };
            work = work.saturating_add(1);
        }
        Ok(work)
    }

    pub(super) fn enforce_memtable_limit(&self, state: &mut MetricState) -> Result<()> {
        let Some(store) = &self.chunk_store else {
            if state.memtable_bytes > self.memtable_limit_bytes {
                bail!("metric memtable exceeded its limit without a disk chunk store");
            }
            return Ok(());
        };
        while state.memtable_bytes > self.memtable_limit_bytes {
            let series_id = state
                .series
                .iter()
                .find(|(_, series)| !series.chunks.is_empty())
                .map(|(series_id, _)| series_id.clone())
                .context("metric memtable accounting exceeds limit without resident chunks")?;
            let series = state
                .series
                .get_mut(&series_id)
                .context("selected metric series disappeared")?;
            let resident_bytes = series.chunks[0].materialized_bytes;
            let sealed = store.write(&series.series_id, &series.chunks[0])?;
            series.chunks.remove(0);
            series.resident_bytes = series.resident_bytes.saturating_sub(resident_bytes);
            series.sealed_chunks.push(sealed);
            state.memtable_bytes = state.memtable_bytes.saturating_sub(resident_bytes);
        }
        Ok(())
    }
}
