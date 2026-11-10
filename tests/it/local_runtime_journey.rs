use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use sift::{router, ServiceState};
use tower::ServiceExt;

fn logs_payload(count: usize) -> serde_json::Value {
    let now = chrono::Utc::now().timestamp_nanos_opt().unwrap() as u64;
    let records: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            serde_json::json!({
                "timeUnixNano": now.to_string(),
                "severityText": "INFO",
                "body": {"stringValue": format!("restart-journey-{i}")},
                "attributes": [
                    {"key": "sift.event_id", "value": {"stringValue": format!("journey-{i}")}}
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
    project: Option<&str>,
    payload: &serde_json::Value,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/logs")
        .header("content-type", "application/json");
    if let Some(project) = project {
        builder = builder.header("x-sift-project", project);
    }
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

#[tokio::test]
async fn ingested_logs_survive_restart_with_identical_records() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    let app = router(state.clone());

    let response = post_logs(&app, Some("journey"), &logs_payload(5)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(
        body["partialSuccess"]["rejectedLogRecords"]
            .as_u64()
            .unwrap_or(0),
        0,
        "no record may be rejected: {body}"
    );

    let before = state.journal().query(Default::default()).unwrap();
    assert_eq!(before.len(), 5, "all five records must be stored");
    for row in &before {
        assert_eq!(row.event.project, "journey");
    }
    for i in 0..5 {
        let needle = format!("restart-journey-{i}");
        let matches = before
            .iter()
            .filter(|row| serde_json::to_string(&row.event).unwrap().contains(&needle))
            .count();
        assert_eq!(matches, 1, "record {i} must be stored exactly once");
    }
    assert!(
        before
            .windows(2)
            .all(|pair| pair[0].cursor < pair[1].cursor),
        "cursors must strictly increase"
    );

    state.finish_drain().await.unwrap();
    drop(app);
    drop(state);

    let reopened = Arc::new(ServiceState::open(temp.path()).unwrap());
    let after = reopened.journal().query(Default::default()).unwrap();
    assert_eq!(before, after, "records changed across restart");
}

#[tokio::test]
async fn ingest_without_a_project_is_rejected_and_stores_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    let app = router(state.clone());

    let response = post_logs(&app, None, &logs_payload(1)).await;
    let status = response.status();
    let body = json_body(response).await;
    eprintln!("status={status} body={body}");
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.to_string().contains("missing_project"),
        "error must name missing_project: {body}"
    );
    assert_eq!(
        state.journal().query(Default::default()).unwrap().len(),
        0,
        "a request without a project must store nothing"
    );
}
