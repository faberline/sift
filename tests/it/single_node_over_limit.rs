use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use sift::{ingest::IngestLimits, router, EventQuery, ServiceState};
use tower::ServiceExt;

fn logs_payload(batch: usize, count: usize) -> serde_json::Value {
    let now = chrono::Utc::now().timestamp_nanos_opt().unwrap() as u64;
    let records: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            serde_json::json!({
                "timeUnixNano": now.to_string(),
                "severityText": "INFO",
                "body": {"stringValue": format!("over-limit-{batch}-{i}")},
                "attributes": [
                    {"key": "sift.event_id", "value": {"stringValue": format!("over-limit-{batch}-{i}")}}
                ]
            })
        })
        .collect();
    serde_json::json!({
        "resourceLogs": [{
            "resource": {"attributes": [
                {"key": "service.name", "value": {"stringValue": "journey"}}
            ]},
            "scopeLogs": [{"logRecords": records}]
        }]
    })
}

async fn post_logs(
    app: &axum::Router,
    project: &str,
    payload: &serde_json::Value,
) -> axum::response::Response {
    let builder = Request::builder()
        .method("POST")
        .uri("/v1/logs")
        .header("content-type", "application/json")
        .header("x-sift-project", project);
    app.clone()
        .oneshot(
            builder
                .body(Body::from(serde_json::to_vec(payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .expect("read response body");
    serde_json::from_slice(&bytes).expect("response JSON")
}

fn read_all(state: &ServiceState) -> Vec<sift::StoredEvent> {
    state
        .journal()
        .query(EventQuery {
            limit: 10_000,
            ..Default::default()
        })
        .unwrap()
}

fn quota_limits() -> IngestLimits {
    IngestLimits {
        max_items_per_project_window: 200,
        quota_window_secs: 3600,
        ..IngestLimits::default()
    }
}

#[tokio::test]
async fn above_the_quota_the_server_rejects_with_retry_after_and_stores_exactly_the_acknowledged_items(
) {
    let temp = tempfile::tempdir().unwrap();
    let state =
        Arc::new(ServiceState::open_with_ingest_limits(temp.path(), quota_limits()).unwrap());
    let app = router(state.clone());

    let mut acknowledged = 0usize;
    let mut accepted_batches = 0usize;
    let mut rejected_batches = 0usize;
    let mut first_rejected: Option<usize> = None;
    for batch in 0..10 {
        let response = post_logs(&app, "single-node", &logs_payload(batch, 50)).await;
        match response.status() {
            StatusCode::OK => {
                acknowledged += 50;
                accepted_batches += 1;
            }
            StatusCode::TOO_MANY_REQUESTS => {
                rejected_batches += 1;
                if first_rejected.is_none() {
                    first_rejected = Some(batch);
                    let retry_after: u64 = response.headers()["retry-after"]
                        .to_str()
                        .unwrap()
                        .parse()
                        .unwrap();
                    assert!(retry_after >= 1, "retry-after must be at least 1");
                    let body = json_body(response).await;
                    assert_eq!(body["error"], "project_quota_exceeded");
                }
            }
            status => panic!("unexpected status {status} for batch {batch}"),
        }
    }

    assert!(accepted_batches >= 1, "the first batches must be admitted");
    assert!(rejected_batches >= 1, "the quota must reject some batches");
    assert_eq!(accepted_batches + rejected_batches, 10);
    assert_eq!(acknowledged, accepted_batches * 50);
    assert!(acknowledged <= 200);

    let stored = read_all(&state);
    assert_eq!(stored.len(), acknowledged);

    let ids: HashSet<String> = stored
        .iter()
        .map(|row| row.event.event_id.clone())
        .collect();
    assert_eq!(ids.len(), stored.len(), "every stored record is unique");

    let first_rejected = first_rejected.expect("a rejection happened");
    let rejected_prefix = format!("over-limit-{first_rejected}-");
    assert!(
        stored
            .iter()
            .all(|row| !row.event.event_id.starts_with(&rejected_prefix)),
        "no record of a rejected batch is stored"
    );
}

#[tokio::test]
async fn below_the_quota_every_batch_is_acknowledged_and_stored() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(
        ServiceState::open_with_ingest_limits(temp.path(), IngestLimits::default()).unwrap(),
    );
    let app = router(state.clone());

    for batch in 0..10 {
        let response = post_logs(&app, "single-node", &logs_payload(batch, 50)).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "batch {batch} must be admitted below the quota"
        );
    }

    assert_eq!(read_all(&state).len(), 500);
}

#[tokio::test]
async fn the_rejection_is_scoped_to_one_project_and_is_retryable() {
    let temp = tempfile::tempdir().unwrap();
    let state =
        Arc::new(ServiceState::open_with_ingest_limits(temp.path(), quota_limits()).unwrap());
    let app = router(state.clone());

    let mut saw_rejection = false;
    for batch in 0..10 {
        let response = post_logs(&app, "single-node", &logs_payload(batch, 50)).await;
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            assert!(
                response.headers().contains_key("retry-after"),
                "the rejection carries retry-after"
            );
            saw_rejection = true;
            break;
        }
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert!(saw_rejection, "the single-node project must hit its quota");

    let response = post_logs(&app, "other-project", &logs_payload(100, 10)).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the quota is per project"
    );

    let other_stored = read_all(&state)
        .iter()
        .filter(|row| row.event.project == "other-project")
        .count();
    assert_eq!(other_stored, 10);
}
