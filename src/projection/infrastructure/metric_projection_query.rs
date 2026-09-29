//! Answering a metric query within its source point and byte budgets: matching
//! series, aggregation, rollups and paging.

use std::ops::Bound::{Excluded, Unbounded};

use anyhow::{bail, Context, Result};

use crate::projection::domain::metric::{
    MetricPage, MetricQuery, MetricSeriesResultV1, MAX_METRIC_QUERY_SOURCE_BYTES,
    MAX_METRIC_QUERY_SOURCE_POINTS, MAX_ROLLUP_LOOKBACK_NANOS,
};
use crate::projection::domain::metric_aggregation::{
    aggregate, make_rollups, merge_histograms, semantic_total,
};
use crate::projection::domain::metric_series::{metric_series_matches, point_is_in_range};
use crate::projection::infrastructure::metric_projection::MetricProjection;

impl MetricProjection {
    pub fn query(&self, query: &MetricQuery) -> Result<MetricPage> {
        self.query_with_source_limits(
            query,
            MAX_METRIC_QUERY_SOURCE_POINTS,
            MAX_METRIC_QUERY_SOURCE_BYTES,
        )
    }

    #[cfg(test)]
    pub(super) fn query_with_source_point_limit(
        &self,
        query: &MetricQuery,
        source_point_limit: usize,
    ) -> Result<MetricPage> {
        self.query_with_source_limits(query, source_point_limit, MAX_METRIC_QUERY_SOURCE_BYTES)
    }

    pub(super) fn query_with_source_limits(
        &self,
        query: &MetricQuery,
        source_point_limit: usize,
        source_byte_limit: usize,
    ) -> Result<MetricPage> {
        if source_point_limit == 0 || source_byte_limit == 0 {
            bail!("metric query source point and byte limits must be non-zero");
        }
        let (start, end) = query.validate()?;
        let state = self.state.read().expect("metric projection lock poisoned");
        let start_series = query
            .after_series_id
            .as_ref()
            .map_or(Unbounded, |series_id| Excluded(series_id.clone()));
        let scan_start = start.map(|start| start.saturating_sub(MAX_ROLLUP_LOOKBACK_NANOS));
        let mut candidates = Vec::new();
        for series in state
            .series
            .range((start_series, Unbounded))
            .map(|(_, series)| series)
            .filter(|series| metric_series_matches(series, query))
        {
            if self.scan_point_count(series, start, end)? > 0 {
                candidates.push(series);
            }
        }
        let has_more = candidates.len() > query.limit;
        let selected = candidates.into_iter().take(query.limit).collect::<Vec<_>>();
        let scanned_points = selected.iter().try_fold(0_usize, |total, series| {
            total
                .checked_add(self.scan_point_count(series, scan_start, end)?)
                .context("metric query source point budget overflowed")
        })?;
        if scanned_points > source_point_limit {
            bail!(
                "metric query would scan {scanned_points} source points; the limit is {source_point_limit}"
            );
        }
        let scanned_bytes = selected.iter().try_fold(0_usize, |total, series| {
            total
                .checked_add(self.scan_materialized_bytes(series, scan_start, end)?)
                .context("metric query source byte budget overflowed")
        })?;
        if scanned_bytes > source_byte_limit {
            bail!(
                "metric query would materialize {scanned_bytes} source bytes; the limit is {source_byte_limit}"
            );
        }
        let mut results = Vec::with_capacity(query.limit);
        let mut last_scanned_series_id = None;
        for series in selected {
            last_scanned_series_id = Some(series.series_id.clone());
            let scan_points = self.points_for_scan(series, scan_start, end)?;
            let points = scan_points
                .iter()
                .filter(|point| point_is_in_range(point, start, end))
                .cloned()
                .collect::<Vec<_>>();
            if points.is_empty() {
                continue;
            }
            let numeric_points = points
                .iter()
                .filter(|point| !point.stale)
                .cloned()
                .collect::<Vec<_>>();
            let (aggregate, reset_count, histogram) = if numeric_points.is_empty() {
                (None, 0, None)
            } else {
                let (semantic_total, reset_count) =
                    semantic_total(series.temporality, &numeric_points);
                (
                    aggregate(query.aggregation, semantic_total, &numeric_points),
                    reset_count,
                    merge_histograms(&numeric_points)?,
                )
            };
            let rollups = make_rollups(&scan_points)?
                .into_iter()
                .filter(|rollup| start.is_none_or(|start| rollup.end_time_unix_nano > start))
                .filter(|rollup| end.is_none_or(|end| rollup.start_time_unix_nano < end))
                .collect();
            results.push(MetricSeriesResultV1 {
                series_id: series.series_id.clone(),
                project: series.project.clone(),
                environment: series.environment.clone(),
                name: series.name.clone(),
                unit: series.unit.clone(),
                temporality: series.temporality,
                resource: series.resource.clone(),
                attributes: series.attributes.clone(),
                overflow: series.overflow,
                points,
                aggregate,
                histogram,
                reset_count,
                rollups,
            });
        }
        Ok(MetricPage {
            next_series_id: last_scanned_series_id,
            series: results,
            projection_cursor: state.projection_cursor,
            has_more,
            overflowed_series: state.overflowed_identities.len() as u64,
            overflowed_points: state.overflowed_points,
        })
    }
}
