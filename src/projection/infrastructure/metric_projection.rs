//! The metric projection: its limits, construction with or without an on-disk
//! chunk store, and the counters maintenance and tests read.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use anyhow::{bail, Result};

use crate::projection::domain::metric::{
    DEFAULT_METRIC_CARDINALITY_LIMIT, DEFAULT_METRIC_MEMTABLE_BYTES,
    DEFAULT_RETAINED_POINTS_PER_SERIES, PROJECTION_METRIC_STORE,
};
use crate::projection::domain::metric_series::MetricState;
use crate::projection::infrastructure::metric_chunk_store::MetricChunkStore;

pub struct MetricProjection {
    pub(super) state: RwLock<MetricState>,
    pub(super) cardinality_limit: usize,
    pub(super) retained_points_per_series: usize,
    pub(super) memtable_limit_bytes: usize,
    pub(super) chunk_store: Option<MetricChunkStore>,
    pub(super) maintenance_work_points: AtomicU64,
}

impl MetricProjection {
    pub fn new() -> Self {
        Self::with_limits(
            DEFAULT_METRIC_CARDINALITY_LIMIT,
            DEFAULT_RETAINED_POINTS_PER_SERIES,
        )
        .expect("default metric limits are valid")
    }

    pub fn with_limits(
        cardinality_limit: usize,
        retained_points_per_series: usize,
    ) -> Result<Self> {
        Self::build(
            cardinality_limit,
            retained_points_per_series,
            usize::MAX,
            None,
        )
    }

    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_limits(
            data_dir,
            DEFAULT_METRIC_CARDINALITY_LIMIT,
            DEFAULT_RETAINED_POINTS_PER_SERIES,
            DEFAULT_METRIC_MEMTABLE_BYTES,
        )
    }

    #[doc(hidden)]
    pub fn open_with_limits(
        data_dir: impl AsRef<Path>,
        cardinality_limit: usize,
        retained_points_per_series: usize,
        memtable_limit_bytes: usize,
    ) -> Result<Self> {
        let chunk_store = MetricChunkStore::open(
            data_dir
                .as_ref()
                .join("indexes")
                .join(PROJECTION_METRIC_STORE)
                .join("chunks"),
        )?;
        Self::build(
            cardinality_limit,
            retained_points_per_series,
            memtable_limit_bytes,
            Some(chunk_store),
        )
    }

    fn build(
        cardinality_limit: usize,
        retained_points_per_series: usize,
        memtable_limit_bytes: usize,
        chunk_store: Option<MetricChunkStore>,
    ) -> Result<Self> {
        if cardinality_limit == 0 || retained_points_per_series == 0 {
            bail!("metric cardinality and retention limits must be non-zero");
        }
        if memtable_limit_bytes == 0 {
            bail!("metric memtable limit must be non-zero");
        }
        Ok(Self {
            state: RwLock::new(MetricState::default()),
            cardinality_limit,
            retained_points_per_series,
            memtable_limit_bytes,
            chunk_store,
            maintenance_work_points: AtomicU64::new(0),
        })
    }

    #[doc(hidden)]
    pub fn maintenance_work_points(&self) -> u64 {
        self.maintenance_work_points.load(Ordering::Relaxed)
    }

    #[doc(hidden)]
    pub fn memtable_bytes(&self) -> usize {
        self.state
            .read()
            .expect("metric projection lock poisoned")
            .memtable_bytes
    }

    #[doc(hidden)]
    pub fn sealed_chunk_count(&self) -> usize {
        self.state
            .read()
            .expect("metric projection lock poisoned")
            .series
            .values()
            .map(|series| series.sealed_chunks.len())
            .sum()
    }
}

impl Default for MetricProjection {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
