//! The metric projection as a Projection: applying metric events with series
//! admission and overflow, and the snapshot, restore and checkpoint that
//! persist its state.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use service_projection::ProjectionDescriptor;

use crate::projection::application::projection_runtime::Projection;
use crate::projection::domain::metric::{
    parse_time, MetricHistogramV1, MetricPointV1, METRIC_SCHEMA_VERSION, PROJECTION_METRIC_STORE,
};
use crate::projection::domain::metric_memory_size::{
    metric_chunk_memory_bytes, metric_series_memory_bytes,
};
use crate::projection::domain::metric_series::{
    identity_material, make_chunks, point_order, push_metric_point, sha256, MetricState,
    StoredMetricSeries,
};
use crate::projection::infrastructure::metric_projection::MetricProjection;
use crate::shared_kernel::stored_event::StoredEvent;
use crate::SignalKind;

#[derive(Deserialize)]
struct MetricSnapshot {
    state: MetricState,
    cardinality_limit: usize,
    retained_points_per_series: usize,
}

#[derive(Serialize)]
struct MetricSnapshotRef<'a> {
    state: &'a MetricState,
    cardinality_limit: usize,
    retained_points_per_series: usize,
}

#[derive(Default, Deserialize)]
struct MetricPayload {
    #[serde(default)]
    histogram: Option<MetricHistogramV1>,
}

impl Projection for MetricProjection {
    fn descriptor(&self) -> ProjectionDescriptor {
        ProjectionDescriptor {
            name: PROJECTION_METRIC_STORE.into(),
            schema_version: METRIC_SCHEMA_VERSION,
            retention: format!(
                "{} points per series; 60s and 3600s rollups",
                self.retained_points_per_series
            ),
        }
    }

    fn apply_idempotent(&self, stored: &StoredEvent) -> Result<()> {
        if stored.event.signal != SignalKind::Metric {
            return Ok(());
        }
        let event = &stored.event;
        let metric = event
            .metric
            .as_ref()
            .context("metric signal requires a direct metric point")?;
        let payload: MetricPayload =
            serde_json::from_value(event.payload.clone()).context("decode metric payload")?;
        if let Some(histogram) = &payload.histogram {
            histogram.validate()?;
        }
        let timestamp = parse_time(&event.occurred_at)?;
        let exact_id = sha256(
            identity_material(
                &event.project,
                &event.environment,
                metric,
                &event.resource,
                &event.attributes,
                false,
            )?
            .as_bytes(),
        );
        let mut state = self.state.write().expect("metric projection lock poisoned");
        if state.projection_cursor >= stored.cursor {
            return Ok(());
        }
        let known = state.series.contains_key(&exact_id);
        let current_count = state
            .exact_identities
            .get(&event.project)
            .map_or(0, BTreeSet::len);
        let overflow = !known && current_count >= self.cardinality_limit;
        let series_id = if overflow {
            state.overflowed_identities.insert(exact_id.clone());
            state.overflowed_points = state.overflowed_points.saturating_add(1);
            sha256(
                identity_material(
                    &event.project,
                    &event.environment,
                    metric,
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    true,
                )?
                .as_bytes(),
            )
        } else {
            state
                .exact_identities
                .entry(event.project.clone())
                .or_default()
                .insert(exact_id.clone());
            exact_id
        };
        let point = MetricPointV1 {
            cursor: stored.cursor,
            event_id: event.event_id.clone(),
            occurred_at: event.occurred_at.clone(),
            time_unix_nano: timestamp,
            value: metric.value,
            stale: metric.stale,
            histogram: payload.histogram,
            exemplars: metric.exemplars.clone(),
        };
        let (touched, before, after) = {
            let series =
                state
                    .series
                    .entry(series_id.clone())
                    .or_insert_with(|| StoredMetricSeries {
                        series_id,
                        project: event.project.clone(),
                        environment: event.environment.clone(),
                        name: metric.name.clone(),
                        unit: metric.unit.clone(),
                        temporality: metric.temporality,
                        resource: if overflow {
                            BTreeMap::from([("sift.metric.overflow".into(), "true".into())])
                        } else {
                            event.resource.clone()
                        },
                        attributes: if overflow {
                            BTreeMap::new()
                        } else {
                            event.attributes.clone()
                        },
                        overflow,
                        sealed_chunks: Vec::new(),
                        chunks: Vec::new(),
                        point_count: 0,
                        rollups: Vec::new(),
                        resident_bytes: 0,
                    });
            if series.temporality != metric.temporality {
                bail!("metric series temporality changed without changing identity");
            }
            if series.point_count == 0
                && (!series.sealed_chunks.is_empty() || !series.chunks.is_empty())
            {
                series.point_count = series
                    .sealed_chunks
                    .iter()
                    .map(|chunk| chunk.point_count)
                    .chain(series.chunks.iter().map(|chunk| chunk.point_count))
                    .sum();
            }
            let before = series.resident_bytes;
            let last_point = self.last_point(series)?;
            let append_in_order = last_point
                .as_ref()
                .is_none_or(|last| point_order(last, &point).is_le());
            let touched = if append_in_order {
                push_metric_point(series, point)?;
                self.seal_full_chunks(series)?;
                let eviction_work = self.evict_series_to_limit(series, stored.cursor)?;
                series.rollups.clear();
                1_u64.saturating_add(eviction_work)
            } else {
                let mut points = self.all_points(series)?;
                points.push(point);
                points.sort_by(point_order);
                let removed = points.len().saturating_sub(self.retained_points_per_series);
                if removed > 0 {
                    points.drain(..removed);
                }
                let touched = points.len() as u64 + removed as u64;
                if let Some(store) = &self.chunk_store {
                    let old_keys = series
                        .sealed_chunks
                        .iter()
                        .map(|chunk| chunk.key.clone())
                        .collect::<Vec<_>>();
                    store.mark_obsolete(stored.cursor, &old_keys)?;
                }
                series.sealed_chunks.clear();
                series.chunks = make_chunks(&points)?;
                series.point_count = points.len();
                series.rollups.clear();
                series.resident_bytes = metric_series_memory_bytes(series)?;
                self.seal_full_chunks(series)?;
                touched
            };
            let after = series.resident_bytes;
            (touched, before, after)
        };
        state.memtable_bytes = state
            .memtable_bytes
            .saturating_sub(before)
            .saturating_add(after);
        self.maintenance_work_points
            .fetch_add(touched, Ordering::Relaxed);
        state.projection_cursor = state.projection_cursor.max(stored.cursor);
        self.enforce_memtable_limit(&mut state)?;
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<u8>> {
        let state = self.state.read().expect("metric projection lock poisoned");
        serde_json::to_vec(&MetricSnapshotRef {
            state: &state,
            cardinality_limit: self.cardinality_limit,
            retained_points_per_series: self.retained_points_per_series,
        })
        .map_err(Into::into)
    }

    fn restore(&self, bytes: &[u8]) -> Result<()> {
        let snapshot: MetricSnapshot =
            serde_json::from_slice(bytes).context("decode metric projection snapshot")?;
        if snapshot.cardinality_limit != self.cardinality_limit
            || snapshot.retained_points_per_series != self.retained_points_per_series
        {
            bail!("metric projection snapshot limits do not match configured limits");
        }
        let mut state = snapshot.state;
        let mut memtable_bytes = 0_usize;
        let mut projection_cursor = state.projection_cursor;
        for series in state.series.values_mut() {
            if !series.sealed_chunks.is_empty() && self.chunk_store.is_none() {
                bail!("metric snapshot contains disk chunks but projection has no chunk store");
            }
            for sealed in &series.sealed_chunks {
                self.read_sealed_chunk(sealed)?;
            }
            series.point_count = series
                .sealed_chunks
                .iter()
                .map(|chunk| chunk.point_count)
                .chain(series.chunks.iter().map(|chunk| chunk.point_count))
                .sum();
            series.rollups.clear();
            for chunk in &mut series.chunks {
                if chunk.point_count != chunk.points.len() || chunk.point_count == 0 {
                    bail!("resident metric chunk point metadata mismatch");
                }
                let measured = metric_chunk_memory_bytes(&chunk.chunk)?;
                if chunk.materialized_bytes != measured || measured == 0 {
                    bail!("resident metric chunk byte metadata mismatch");
                }
            }
            series.resident_bytes = metric_series_memory_bytes(series)?;
            memtable_bytes = memtable_bytes
                .checked_add(series.resident_bytes)
                .context("metric memtable byte accounting overflowed during restore")?;
            for point in series.chunks.iter().flat_map(|chunk| &chunk.points) {
                projection_cursor = projection_cursor.max(point.cursor);
            }
        }
        state.memtable_bytes = memtable_bytes;
        state.projection_cursor = projection_cursor;
        self.enforce_memtable_limit(&mut state)?;
        *self.state.write().expect("metric projection lock poisoned") = state;
        Ok(())
    }

    fn checkpoint_committed(&self) -> Result<()> {
        let Some(store) = &self.chunk_store else {
            return Ok(());
        };
        let state = self.state.read().expect("metric projection lock poisoned");
        let committed_cursor = state.projection_cursor;
        let referenced = state
            .series
            .values()
            .flat_map(|series| series.sealed_chunks.iter().map(|chunk| chunk.key.clone()))
            .collect::<BTreeSet<_>>();
        drop(state);
        store.cleanup_obsolete(committed_cursor, &referenced)
    }

    fn semantic_digest(&self) -> Result<String> {
        Ok(sha256(&self.snapshot()?))
    }
}
