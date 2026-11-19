//! isolation: no other test runs in this process, so no child process of another case can hold a copy of the data-root lock descriptor.
//! If this test fails, the drain returns before the lock is released.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use sift::{router, DurableJournal, ServiceState};
use tower::ServiceExt;

fn log_record(sequence: usize) -> serde_json::Value {
    serde_json::json!({
        "body": {"stringValue": format!("drain round record {sequence}")},
        "attributes": [{
            "key": "sift.event_id",
            "value": {"stringValue": format!("drain-{sequence}")}
        }]
    })
}

#[tokio::test]
async fn drain_then_drop_releases_the_data_root_lock_at_once() {
    for round in 0..100 {
        let data_dir = tempfile::tempdir().expect("temporary Sift data directory");
        let state = Arc::new(ServiceState::open(data_dir.path()).expect("open Sift state"));
        let app = router(state.clone());

        let payload = serde_json::json!({
            "resourceLogs": [{
                "resource": {"attributes": [{
                    "key": "service.name",
                    "value": {"stringValue": "sift-drain"}
                }]},
                "scopeLogs": [{
                    "logRecords": (0..4).map(log_record).collect::<Vec<_>>()
                }]
            }]
        });
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/logs")
                    .header("content-type", "application/json")
                    .header("x-sift-project", "drain")
                    .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "round {round}: ingest");

        state.start_drain();
        state
            .finish_drain()
            .await
            .unwrap_or_else(|error| panic!("round {round}: finish drain: {error:?}"));

        drop(response);
        drop(app);
        drop(state);
        let journal = DurableJournal::open(data_dir.path())
            .unwrap_or_else(|error| panic!("round {round}: reopen after drain failed: {error:?}"));
        drop(journal);
    }
}
