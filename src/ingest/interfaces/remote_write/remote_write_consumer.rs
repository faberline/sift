//! Converting a validated Prometheus WriteRequest into metric events for the
//! admitted project, with its metadata, exemplars and stale markers.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use chrono::{TimeZone, Utc};
use metrics_remote_write::{proto as remote, PROMETHEUS_STALE_NAN_BITS};
use sha2::{Digest, Sha256};

use crate::{
    AttributeValue, EventEnvelope, MetricExemplar, MetricPoint, MetricTemporality, SignalKind,
};

#[derive(Clone, Debug)]
pub struct DecodedWrite {
    pub events: Vec<EventEnvelope>,
    pub project: String,
}

pub fn decode_remote_write(body: &[u8], admitted_project: Option<&str>) -> Result<DecodedWrite> {
    let write = metrics_remote_write::decode_write_request(body)
        .context("validate Prometheus WriteRequest")?;
    metrics_remote_write::RemoteWriteConsumer::consume(
        &SiftRemoteWriteConsumer { admitted_project },
        write,
    )
}

struct SiftRemoteWriteConsumer<'a> {
    admitted_project: Option<&'a str>,
}

impl metrics_remote_write::RemoteWriteConsumer for SiftRemoteWriteConsumer<'_> {
    type Output = DecodedWrite;
    type Error = anyhow::Error;

    fn consume(&self, write: metrics_remote_write::ValidatedWrite) -> Result<Self::Output> {
        let admitted_project = self.admitted_project;
        let request = write.into_inner();
        let metadata = request
            .metadata
            .into_iter()
            .map(|item| (item.metric_family_name, item.unit))
            .collect::<BTreeMap<_, _>>();
        let mut events = Vec::new();
        let mut request_project: Option<String> = admitted_project.map(str::to_owned);
        for series in request.timeseries {
            let labels = series
                .labels
                .into_iter()
                .map(|label| (label.name, label.value))
                .collect::<BTreeMap<_, _>>();
            let metric_name = labels
                .get("__name__")
                .filter(|name| !name.is_empty())
                .context("remote write series requires __name__ label")?;
            let project = labels
                .get("project")
                .or_else(|| labels.get("sift.project"))
                .map(String::as_str)
                .or(admitted_project)
                .context("remote write series requires project label or x-sift-project header")?;
            if admitted_project.is_some_and(|admitted| admitted != project) {
                bail!("series project `{project}` does not match admitted project");
            }
            if request_project
                .as_deref()
                .is_some_and(|known| known != project)
            {
                bail!("one remote write request cannot mix projects");
            }
            request_project = Some(project.to_owned());
            let environment = labels
                .get("environment")
                .or_else(|| labels.get("deployment.environment.name"))
                .map(String::as_str)
                .unwrap_or("default");
            for sample in series.samples {
                let stale = sample.value.to_bits() == PROMETHEUS_STALE_NAN_BITS;
                let occurred_at = Utc
                    .timestamp_millis_opt(sample.timestamp)
                    .single()
                    .context("remote write timestamp is outside the supported range")?
                    .to_rfc3339();
                let identity = serde_json::to_vec(&serde_json::json!({
                    "project": project,
                    "labels": labels,
                    "timestamp": sample.timestamp,
                    "value_bits": sample.value.to_bits()
                }))?;
                let event_id = format!("prom-rw-{}", hex::encode(&Sha256::digest(identity)[..16]));
                let mut event = EventEnvelope::for_project(
                    project,
                    environment,
                    event_id,
                    SignalKind::Metric,
                    serde_json::json!({"prometheus_remote_write": "1.0"}),
                );
                event.occurred_at = occurred_at.clone();
                event.observed_at = occurred_at;
                event
                    .resource
                    .insert("telemetry.sdk.name".into(), "prometheus".into());
                for (name, value) in &labels {
                    if matches!(
                        name.as_str(),
                        "__name__"
                            | "project"
                            | "sift.project"
                            | "environment"
                            | "deployment.environment.name"
                    ) {
                        continue;
                    }
                    if name.starts_with("service.")
                        || name.starts_with("cloud.")
                        || name.starts_with("k8s.")
                    {
                        event.resource.insert(name.clone(), value.clone());
                    } else {
                        event
                            .attributes
                            .insert(name.clone(), AttributeValue::String(value.clone()));
                    }
                }
                let exemplars = series
                    .exemplars
                    .iter()
                    .filter(|exemplar| exemplar.timestamp == sample.timestamp)
                    .filter_map(to_metric_exemplar)
                    .collect();
                event.metric = Some(MetricPoint {
                    name: metric_name.clone(),
                    value: if stale { 0.0 } else { sample.value },
                    stale,
                    unit: metadata
                        .get(metric_name)
                        .filter(|unit| !unit.is_empty())
                        .cloned(),
                    temporality: if metric_name.ends_with("_total") {
                        MetricTemporality::Cumulative
                    } else {
                        MetricTemporality::Gauge
                    },
                    exemplars,
                });
                events.push(event);
            }
        }
        Ok(DecodedWrite {
            events,
            project: request_project.context("remote write request has no project")?,
        })
    }
}

fn to_metric_exemplar(exemplar: &remote::Exemplar) -> Option<MetricExemplar> {
    let labels = exemplar
        .labels
        .iter()
        .map(|label| (label.name.as_str(), label.value.as_str()))
        .collect::<BTreeMap<_, _>>();
    Some(MetricExemplar {
        value: exemplar.value,
        trace_id: labels.get("trace_id")?.to_string(),
        span_id: labels.get("span_id")?.to_string(),
    })
}
