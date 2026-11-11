use std::{sync::Arc, time::Duration};

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use sift::{router, EventEnvelope, ServiceState, SignalKind};
use tower::ServiceExt;

fn log(id: &str, trace_id: &str) -> EventEnvelope {
    let mut event = EventEnvelope::for_project(
        "project-a",
        "prod",
        id,
        SignalKind::Log,
        serde_json::json!({"message": "checkout failed"}),
    );
    event.trace_id = Some(trace_id.into());
    event
        .resource
        .insert("service.name".into(), "checkout".into());
    event
        .resource
        .insert("service.version".into(), "2026.8.30".into());
    event
}

fn span(trace_id: &str) -> EventEnvelope {
    let mut event = EventEnvelope::for_project(
        "project-a",
        "prod",
        "span-1",
        SignalKind::Span,
        serde_json::json!({
            "name": "checkout",
            "start_time_unix_nano": 10,
            "end_time_unix_nano": 20,
            "status": {"code": "error"}
        }),
    );
    event.trace_id = Some(trace_id.into());
    event.span_id = Some("root".into());
    event
        .resource
        .insert("service.name".into(), "checkout".into());
    event
        .resource
        .insert("service.version".into(), "2026.8.30".into());
    event
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn post(app: &axum::Router, uri: &str, value: serde_json::Value) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&value).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn tail_correlate_and_services_share_the_phase_one_model() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    state.journal().append(log("log-1", "trace-1")).unwrap();
    state.journal().append(span("trace-1")).unwrap();
    let app = router(state);

    let tail = post(
        &app,
        "/api/v1/logs/tail",
        serde_json::json!({
            "version": 1,
            "project": "project-a",
            "environment": "prod",
            "after_cursor": null,
            "wait_ms": 0,
            "limit": 10
        }),
    )
    .await;
    assert_eq!(tail.status(), StatusCode::OK);
    let tail = body_json(tail).await;
    assert_eq!(tail["data"]["records"][0]["event_id"], "log-1");
    assert!(tail["next_cursor"].as_str().unwrap().starts_with("logs:"));

    let correlated = post(
        &app,
        "/api/v1/correlate",
        serde_json::json!({
            "version": 1,
            "project": "project-a",
            "environment": "prod",
            "trace_id": "trace-1",
            "limit": 10
        }),
    )
    .await;
    assert_eq!(correlated.status(), StatusCode::OK);
    let correlated = body_json(correlated).await;
    assert_eq!(correlated["logs"][0]["event_id"], "log-1");
    assert_eq!(correlated["traces"][0]["trace_id"], "trace-1");
    assert_eq!(correlated["partial"], false);

    let services = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/services?project=project-a&environment=prod")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(services.status(), StatusCode::OK);
    let services = body_json(services).await;
    assert_eq!(services["services"][0]["name"], "checkout");
    assert_eq!(
        services["services"][0]["environments"],
        serde_json::json!(["prod"])
    );
    assert_eq!(
        services["services"][0]["versions"],
        serde_json::json!(["2026.8.30"])
    );
    assert_eq!(
        services["services"][0]["signals"],
        serde_json::json!(["logs", "traces"])
    );
}

#[tokio::test]
async fn log_tail_reconnects_after_its_cursor_without_crossing_projects() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    state.journal().append(log("first", "trace-1")).unwrap();
    let app = router(Arc::clone(&state));

    let tail_page = async |after_cursor: Option<&str>| {
        let response = post(
            &app,
            "/api/v1/logs/tail",
            serde_json::json!({
                "version": 1,
                "project": "project-a",
                "environment": "prod",
                "filter": {"op": "eq", "field": "trace_id", "value": "trace-1"},
                "after_cursor": after_cursor,
                "wait_ms": 0,
                "limit": 2
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let page = body_json(response).await;
        assert_eq!(page["partial"], false);
        page
    };

    let first = tail_page(None).await;
    assert_eq!(first["data"]["records"].as_array().unwrap().len(), 1);
    assert_eq!(first["data"]["records"][0]["event_id"], "first");
    let first_cursor = first["next_cursor"].as_str().expect("first tail cursor");

    let mut foreign = log("foreign", "trace-1");
    foreign.project = "project-b".into();
    state.journal().append(foreign).unwrap();
    state.journal().append(log("filtered", "trace-2")).unwrap();
    state.journal().append(log("new", "trace-1")).unwrap();

    let resumed = tail_page(Some(first_cursor)).await;
    assert_eq!(resumed["data"]["records"].as_array().unwrap().len(), 1);
    assert_eq!(resumed["data"]["records"][0]["event_id"], "new");
    let resumed_cursor = resumed["next_cursor"]
        .as_str()
        .expect("resumed tail cursor");
    assert_ne!(resumed_cursor, first_cursor);

    let empty = tail_page(Some(resumed_cursor)).await;
    assert!(empty["data"]["records"].as_array().unwrap().is_empty());
    assert_eq!(empty["next_cursor"], resumed_cursor);
}

#[tokio::test]
async fn async_query_job_is_persisted_and_read_after_restart() {
    let temp = tempfile::tempdir().unwrap();
    let query_id;
    {
        let state = Arc::new(ServiceState::open(temp.path()).unwrap());
        state.journal().append(log("log-1", "trace-1")).unwrap();
        let app = router(state);
        let accepted = post(
            &app,
            "/api/v1/query",
            serde_json::json!({
                "version": 1,
                "project": "project-a",
                "signal": {"kind": "logs"},
                "mode": "async"
            }),
        )
        .await;
        assert_eq!(accepted.status(), StatusCode::ACCEPTED);
        let accepted = body_json(accepted).await;
        query_id = accepted["query_id"].as_str().unwrap().to_string();

        for _ in 0..100 {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/v1/queries/{query_id}?project=project-a"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            if body_json(response).await["status"] == "succeeded" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    let reopened = router(Arc::new(ServiceState::open(temp.path()).unwrap()));
    let response = reopened
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/queries/{query_id}?project=project-a"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = body_json(response).await;
    assert_eq!(response["status"], "succeeded");
    assert_eq!(
        response["result"]["data"]["records"][0]["event_id"],
        "log-1"
    );
}

#[tokio::test]
async fn log_tail_cursor_survives_a_restart_without_crossing_projects() {
    let temp = tempfile::tempdir().unwrap();
    let tail_body = |after_cursor: Option<&str>| {
        serde_json::json!({
            "version": 1,
            "project": "project-a",
            "environment": "prod",
            "filter": {"op": "eq", "field": "trace_id", "value": "trace-1"},
            "after_cursor": after_cursor,
            "wait_ms": 0,
            "limit": 10
        })
    };
    let first_cursor: String;
    {
        let state = Arc::new(ServiceState::open(temp.path()).unwrap());
        state.journal().append(log("first", "trace-1")).unwrap();
        let mut foreign = log("foreign", "trace-1");
        foreign.project = "project-b".into();
        state.journal().append(foreign).unwrap();
        let app = router(Arc::clone(&state));

        let page = post(&app, "/api/v1/logs/tail", tail_body(None)).await;
        assert_eq!(page.status(), StatusCode::OK);
        let page = body_json(page).await;
        assert_eq!(page["partial"], false);
        let records = page["data"]["records"].as_array().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["event_id"], "first");
        first_cursor = page["next_cursor"]
            .as_str()
            .expect("first tail cursor")
            .to_string();
    }

    let reopened = Arc::new(ServiceState::open(temp.path()).unwrap());
    reopened.journal().append(log("second", "trace-1")).unwrap();
    let mut foreign_again = log("foreign-2", "trace-1");
    foreign_again.project = "project-b".into();
    reopened.journal().append(foreign_again).unwrap();
    let app = router(Arc::clone(&reopened));

    let page = post(
        &app,
        "/api/v1/logs/tail",
        tail_body(Some(first_cursor.as_str())),
    )
    .await;
    assert_eq!(page.status(), StatusCode::OK);
    let page = body_json(page).await;
    assert_eq!(page["partial"], false);
    let records = page["data"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["event_id"], "second");
    let second_cursor = page["next_cursor"]
        .as_str()
        .expect("second tail cursor")
        .to_string();
    assert_ne!(second_cursor, first_cursor);

    let page = post(
        &app,
        "/api/v1/logs/tail",
        tail_body(Some(second_cursor.as_str())),
    )
    .await;
    assert_eq!(page.status(), StatusCode::OK);
    let page = body_json(page).await;
    assert!(page["data"]["records"].as_array().unwrap().is_empty());
    assert_eq!(page["next_cursor"], second_cursor);
}
