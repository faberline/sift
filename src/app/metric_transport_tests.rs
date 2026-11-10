//! Native OTLP buckets and scraped classic buckets give the same quantiles.
use crate::app::ServiceState;
use axum::{body::Body, http::Request};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc};
use tower::ServiceExt;
async fn ingest(state: Arc<ServiceState>, project: &str, body: Value) {
    let response = crate::router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/metrics")
                .header("content-type", "application/json")
                .header("x-sift-project", project)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(response.status().is_success());
}
#[tokio::test]
async fn scraped_and_otlp_histograms_keep_counts_windows_and_p95_p99() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(dir.path()).unwrap());
    let now = chrono::Utc::now().timestamp_nanos_opt().unwrap();
    let resource = BTreeMap::from([("service.name".into(), "checkout".into())]);
    for (at, factor) in [(now - 60_000_000_000, 1), (now, 2)] {
        let native = json!({"resourceMetrics":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"checkout"}}]},
        "scopeMetrics":[{"metrics":[{"name":"duration","histogram":{"aggregationTemporality":2,"dataPoints":[{
            "startTimeUnixNano":(now-600_000_000_000).to_string(),"timeUnixNano":at.to_string(),
            "count":(factor*4).to_string(),"sum":factor as f64*2.5,
            "explicitBounds":[0.1,1.0],"bucketCounts":[factor,factor*2,factor]
        }]}}]}]}]});
        ingest(state.clone(), "otlp", native).await;
        let text=format!("# TYPE duration histogram\nduration_bucket{{le=\"0.1\"}} {factor}\nduration_bucket{{le=\"1\"}} {}\nduration_bucket{{le=\"+Inf\"}} {}\nduration_sum {}\nduration_count {}\n",factor*3,factor*4,factor as f64*2.5,factor*4);
        let samples = crate::collector::domain::prometheus_text::parse_text(&text).unwrap();
        let scraped =
            crate::collector::infrastructure::metric_scraper::encode(&samples, &resource, at)
                .unwrap();
        ingest(state.clone(), "scraped", scraped).await;
    }
    for quantile in [0.5, 0.95, 0.99] {
        let mut responses = Vec::new();
        for project in ["otlp", "scraped"] {
            let request=serde_json::from_value(json!({"version":2,"project":project,
                "time_range":{"end":crate::query::nanos_rfc3339(now).unwrap()},
                "signal":{"kind":"metrics","expression":format!("histogram_quantile({quantile}, sum by(le) (rate(duration_bucket[2m])))")}})).unwrap();
            let response = state.execute_query_v2(None, &request).unwrap();
            assert!(!response.response.partial);
            responses.push(response.response.data["result"][0]["value"][1].clone());
        }
        assert_eq!(responses[0], responses[1], "quantile {quantile}");
    }
}
