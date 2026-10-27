//! Query-owned inputs for Prometheus evaluation.

use crate::shared_kernel::event::AttributeValue;
use std::collections::BTreeMap;

pub(in crate::query) const PROM_MAX_SERIES: usize = 1_000;

pub(in crate::query) struct PromPoint {
    pub(in crate::query) time_unix_nano: i64,
    pub(in crate::query) value: f64,
    pub(in crate::query) stale: bool,
}

pub(in crate::query) struct PromSeries {
    pub(in crate::query) name: String,
    pub(in crate::query) resource: BTreeMap<String, String>,
    pub(in crate::query) attributes: BTreeMap<String, AttributeValue>,
    pub(in crate::query) points: Vec<PromPoint>,
}

pub(in crate::query) struct PromMetricPage {
    pub(in crate::query) series: Vec<PromSeries>,
    pub(in crate::query) has_more: bool,
}
