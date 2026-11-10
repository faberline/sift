//! One metric representation and event identity for every OTLP transport.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde_json::json;
use transport_otlp::OtlpSignal;

use super::otlp_event_builder::{apply_common_attributes, base_event};
use crate::ingest::application::event_identity::stable_id;
use crate::shared_kernel::projection_metric::{HistogramKind, MetricHistogramV1};
use crate::{
    AttributeValue, InstrumentationScope, MetricExemplar, MetricPoint, MetricTemporality,
    OperationalEventV2,
};

pub(super) struct MetricInput {
    pub name: String,
    pub unit: Option<String>,
    pub value: f64,
    pub temporality: MetricTemporality,
    pub monotonic: bool,
    pub start: u64,
    pub end: u64,
    pub flags: u32,
    pub attributes: BTreeMap<String, AttributeValue>,
    pub exemplars: Vec<MetricExemplar>,
    pub histogram: Option<MetricHistogramV1>,
}

pub(super) fn metric_event(
    project: &str,
    resource: &BTreeMap<String, String>,
    scope: Option<InstrumentationScope>,
    mut input: MetricInput,
) -> Result<OperationalEventV2> {
    input.unit = input.unit.filter(|unit| !unit.is_empty());
    if input.name.trim().is_empty() || !input.value.is_finite() {
        bail!("metric requires a name and finite value");
    }
    if input.end == 0 || input.end > i64::MAX as u64 || input.start > input.end {
        bail!("metric timestamp is missing or outside its valid interval");
    }
    if let Some(histogram) = &input.histogram {
        validate_histogram(histogram)?;
    }
    let payload = json!({
        "startTimeUnixNano": input.start,
        "timeUnixNano": input.end,
        "flags": input.flags,
        "isMonotonic": input.monotonic,
        "histogram": input.histogram,
    });
    // Sort resource and point attributes. Include scope and resource so two
    // Pods at one timestamp cannot overwrite each other. Raw transport bytes
    // are not part of identity, so JSON and protobuf retries deduplicate.
    let identity = serde_json::to_string(&json!({
        "resource": resource, "scope": scope, "name": input.name,
        "unit": input.unit, "value": input.value, "temporality": input.temporality,
        "attributes": input.attributes, "exemplars": input.exemplars, "payload": payload,
    }))?;
    let mut event = base_event(
        OtlpSignal::Metrics,
        project,
        resource,
        stable_id("metric", project, &identity),
        input.end,
        input.end,
        payload,
    );
    event.instrumentation_scope = scope;
    event.attributes = input.attributes;
    apply_common_attributes(&mut event);
    event.metric = Some(MetricPoint {
        name: input.name,
        unit: input.unit,
        value: input.value,
        temporality: input.temporality,
        stale: input.flags & 1 != 0,
        exemplars: input.exemplars,
    });
    Ok(event)
}

fn validate_histogram(hist: &MetricHistogramV1) -> Result<()> {
    if !hist.sum.is_finite()
        || hist.explicit_bounds.iter().any(|v| !v.is_finite())
        || hist.min.is_some_and(|v| !v.is_finite())
        || hist.max.is_some_and(|v| !v.is_finite())
        || hist.min.zip(hist.max).is_some_and(|(min, max)| min > max)
    {
        bail!("histogram numbers must be finite and min must not exceed max");
    }
    let total = match hist.kind {
        HistogramKind::Explicit => {
            if hist.bucket_counts.len() != hist.explicit_bounds.len() + 1
                || !hist
                    .explicit_bounds
                    .windows(2)
                    .all(|pair| pair[0] < pair[1])
            {
                bail!("histogram bucket shape or bounds are invalid");
            }
            hist.bucket_counts
                .iter()
                .try_fold(0u64, |total, value| total.checked_add(*value))
        }
        HistogramKind::Exponential => {
            if hist.scale.is_none()
                || !hist.explicit_bounds.is_empty()
                || !hist.bucket_counts.is_empty()
                || !hist.zero_threshold.is_finite()
                || hist.zero_threshold < 0.0
            {
                bail!("exponential histogram shape or zero threshold is invalid");
            }
            hist.positive_bucket_counts
                .iter()
                .chain(&hist.negative_bucket_counts)
                .try_fold(hist.zero_count, |total, count| total.checked_add(*count))
        }
    };
    if total != Some(hist.count) {
        bail!("histogram buckets must sum to count without overflow");
    }
    Ok(())
}

pub(super) fn explicit_histogram(
    count: u64,
    sum: Option<f64>,
    bounds: Vec<f64>,
    buckets: Vec<u64>,
    min: Option<f64>,
    max: Option<f64>,
) -> MetricHistogramV1 {
    MetricHistogramV1 {
        kind: HistogramKind::Explicit,
        count,
        sum: sum.unwrap_or(0.0),
        sum_present: sum.is_some(),
        zero_threshold: 0.0,
        explicit_bounds: bounds,
        bucket_counts: buckets,
        scale: None,
        zero_count: 0,
        positive_offset: 0,
        positive_bucket_counts: Vec::new(),
        negative_offset: 0,
        negative_bucket_counts: Vec::new(),
        min,
        max,
    }
}

#[cfg(test)]
mod tests;
