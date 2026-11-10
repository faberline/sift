//! Transport acknowledgements and resource identities must remain truthful.
use super::ServiceState;
use axum::{body::Body, http::Request};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

async fn post(
    state: Arc<ServiceState>,
    project: &str,
    path: &str,
    body: Value,
) -> axum::response::Response {
    crate::router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .header("x-sift-project", project)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn missing_remote_protection_returns_a_retryable_error_without_an_acknowledgement() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = ServiceState::open(dir.path()).unwrap();
    state.recovery_guard =
        Arc::new(crate::operations::domain::recovery_guard::RecoveryGuard::new(true));
    let state = Arc::new(state);
    for path in ["/v1/logs", "/v1/metrics", "/v1/traces"] {
        let response = post(state.clone(), "p", path, json!({})).await;
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        let bytes = axum::body::to_bytes(response.into_body(), 16_384)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["retryable"], true);
        assert!(body["message"].as_str().unwrap().contains("15 minutes"));
        assert_eq!(state.journal().last_cursor(), 0);
    }
}

#[tokio::test]
async fn identical_otlp_logs_at_one_time_keep_distinct_resource_and_scope_identities() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(dir.path()).unwrap());
    let now = chrono::Utc::now()
        .timestamp_nanos_opt()
        .unwrap()
        .to_string();
    let record = json!({"timeUnixNano":now,"body":{"stringValue":"same body"}});
    let mut resources = Vec::new();
    for pod in ["pod-a", "pod-b"] {
        resources.push(
            json!({"resource":{"attributes":[{"key":"k8s.pod.uid","value":{"stringValue":pod}}]},
            "scopeLogs":[{"scope":{"name":"first"},"logRecords":[record.clone()]},
                         {"scope":{"name":"second"},"logRecords":[record.clone()]}]}),
        );
    }
    let body = json!({"resourceLogs":resources});
    for _ in 0..2 {
        assert!(post(state.clone(), "p", "/v1/logs", body.clone())
            .await
            .status()
            .is_success());
    }
    let response = post(
        state,
        "p",
        "/api/v1/query",
        json!({"version":1,"project":"p",
        "signal":{"kind":"logs"},"limit":100}),
    )
    .await;
    assert!(response.status().is_success());
    let bytes = axum::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let records = value["data"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 4);
    let ids: std::collections::BTreeSet<_> = records
        .iter()
        .map(|r| r["event_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 4);
    assert_eq!(
        records
            .iter()
            .filter(|r| r["resource"]["k8s.pod.uid"] == "pod-a")
            .count(),
        2
    );
}
