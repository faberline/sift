//! Export durable collector progress without reading raw log text into output.
use anyhow::{bail, Result};
use axum::{extract::State, http::StatusCode, routing::get, Router};
use serde::{de::DeserializeOwned, de::IgnoredAny, Deserialize};
use std::{
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

const STATE_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Default, Deserialize)]
#[serde(default)]
struct CheckpointTotals {
    accepted: u64,
    duplicates: u64,
    rejected: u64,
    lost_bytes: u64,
    lost_sources: u64,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct FramingDiagnostic {
    ready: Vec<RawDiagnostic>,
    pending: Option<RawDiagnostic>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct RawDiagnostic {
    enrichment: EnrichmentDiagnostic,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct EnrichmentDiagnostic {
    attributes: ObservationDiagnostic,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct ObservationDiagnostic {
    #[serde(rename = "collector.observed_at")]
    observed_at: Option<crate::AttributeValue>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct DeliveryDiagnostic {
    observed_at: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct MetricQueueDiagnostic {
    batches: Vec<MetricBatchDiagnostic>,
    next: usize,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct MetricBatchDiagnostic {
    resource_metrics: Vec<MetricResourceDiagnostic>,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct MetricResourceDiagnostic {
    scope_metrics: Vec<MetricScopeDiagnostic>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct MetricScopeDiagnostic {
    metrics: Vec<MetricDiagnostic>,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct MetricDiagnostic {
    gauge: MetricPointsDiagnostic,
    sum: MetricPointsDiagnostic,
    histogram: MetricPointsDiagnostic,
    exponential_histogram: MetricPointsDiagnostic,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct MetricPointsDiagnostic {
    data_points: Vec<IgnoredAny>,
}

pub fn collector_metrics_router(checkpoint: PathBuf) -> Router {
    Router::new()
        .route("/metrics", get(metrics))
        .with_state(checkpoint)
}
async fn metrics(State(path): State<PathBuf>) -> Result<String, StatusCode> {
    tokio::task::spawn_blocking(move || render(&path))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}
fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.len() > STATE_LIMIT {
        bail!("collector metric state is not a bounded regular file");
    }
    if metadata.len() == 0 {
        return Ok(T::default());
    }
    // Ignore raw records while streaming. A JSON byte array must not become
    // millions of serde_json::Value allocations on the metrics endpoint.
    let mut reader = BufReader::new(std::fs::File::open(path)?).take(STATE_LIMIT + 1);
    let value = serde_json::from_reader(&mut reader)?;
    if STATE_LIMIT + 1 - reader.limit() > STATE_LIMIT {
        bail!("collector metric state grew beyond its bound");
    }
    Ok(value)
}
fn render(path: &Path) -> Result<String> {
    let checkpoint: CheckpointTotals = load(path)?;
    let framing: FramingDiagnostic = load(&path.with_extension("framing.json"))?;
    let delivery: Vec<DeliveryDiagnostic> = load(&path.with_extension("pending.json"))?;
    let queue: MetricQueueDiagnostic = load(
        &path
            .parent()
            .unwrap_or(Path::new("."))
            .join("metrics/pending-metrics.json"),
    )?;
    if queue.next > queue.batches.len() {
        bail!("collector metric queue offset is invalid");
    }
    let metric_points = queue
        .batches
        .iter()
        .skip(queue.next)
        .flat_map(|b| &b.resource_metrics)
        .flat_map(|r| &r.scope_metrics)
        .flat_map(|s| &s.metrics)
        .map(|m| {
            m.gauge.data_points.len()
                + m.sum.data_points.len()
                + m.histogram.data_points.len()
                + m.exponential_histogram.data_points.len()
        })
        .sum::<usize>();
    let framed = framing.ready.len() + usize::from(framing.pending.is_some());
    let delivered = delivery.len();
    let mut oldest = None;
    for record in framing.ready.iter().chain(framing.pending.iter()) {
        if let Some(crate::AttributeValue::String(value)) =
            &record.enrichment.attributes.observed_at
        {
            let t = chrono::DateTime::parse_from_rfc3339(value)?.timestamp();
            oldest = Some(oldest.map_or(t, |old: i64| old.min(t)));
        }
    }
    for event in &delivery {
        if let Some(value) = &event.observed_at {
            let t = chrono::DateTime::parse_from_rfc3339(value)?.timestamp();
            oldest = Some(oldest.map_or(t, |old: i64| old.min(t)));
        }
    }
    let lag = oldest.map_or(0, |t| {
        chrono::Utc::now().timestamp().saturating_sub(t).max(0)
    });
    let mut output = format!("# TYPE sift_collector_pending_records gauge\nsift_collector_pending_records {}\n# TYPE sift_collector_pending_metric_points gauge\nsift_collector_pending_metric_points {metric_points}\n# TYPE sift_collection_lag_seconds gauge\nsift_collection_lag_seconds {lag}\n", framed.max(delivered));
    for (value, name) in [
        (checkpoint.accepted, "accepted_records"),
        (checkpoint.duplicates, "duplicate_records"),
        (checkpoint.rejected, "rejected_records"),
        (checkpoint.lost_bytes, "lost_bytes"),
        (checkpoint.lost_sources, "lost_sources"),
    ] {
        output.push_str(&format!(
            "# TYPE sift_collector_{name}_total counter\nsift_collector_{name}_total {value}\n"
        ));
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_durable_views_do_not_double_pending_count_or_expose_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checkpoint.json");
        std::fs::write(&path, r#"{"accepted":8,"rejected":2,"lost_bytes":7}"#).unwrap();
        std::fs::write(
            path.with_extension("framing.json"),
            r#"{"ready":[{},{}],"pending":null}"#,
        )
        .unwrap();
        std::fs::write(
            path.with_extension("pending.json"),
            r#"[{"message":"private-log"},{"message":"private-log"}]"#,
        )
        .unwrap();
        let output = render(&path).unwrap();
        assert!(output.contains("sift_collector_pending_records 2\n"));
        assert!(output.contains("sift_collector_lost_bytes_total 7\n"));
        assert!(!output.contains("private-log"));
    }

    #[test]
    fn metric_backlog_excludes_acknowledged_batches_and_raw_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checkpoint.json");
        std::fs::create_dir(dir.path().join("metrics")).unwrap();
        let batch = serde_json::json!({"resourceMetrics":[{"scopeMetrics":[{"metrics":[
            {"gauge":{"dataPoints":[{"attributes":"private-metric"},{}]}},
            {"histogram":{"dataPoints":[{"bucketCounts":[1,2,3]}]}}
        ]}]}]});
        let queue = serde_json::json!({"next":1,"batches":[batch.clone(),batch]});
        let queue_path = dir.path().join("metrics/pending-metrics.json");
        std::fs::write(&queue_path, serde_json::to_vec(&queue).unwrap()).unwrap();
        let output = render(&path).unwrap();
        assert!(output.contains("sift_collector_pending_metric_points 3\n"));
        assert!(!output.contains("private-metric"));
        std::fs::write(&queue_path, r#"{"next":1,"batches":[]}"#).unwrap();
        assert!(render(&path).is_err());
    }
}
