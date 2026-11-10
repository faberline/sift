use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use sift::{router, EventEnvelope, MetricPoint, MetricTemporality, ServiceState, SignalKind};
use tower::ServiceExt;

fn log(id: &str, message: &str) -> EventEnvelope {
    let mut event = EventEnvelope::for_project(
        "project-a",
        "prod",
        id,
        SignalKind::Log,
        serde_json::json!({"message": message}),
    );
    event.severity = Some("ERROR".into());
    event
        .resource
        .insert("service.name".into(), "checkout".into());
    event
}

fn metric() -> EventEnvelope {
    let mut event = EventEnvelope::for_project(
        "project-a",
        "prod",
        "metric-1",
        SignalKind::Metric,
        serde_json::json!({}),
    );
    event
        .resource
        .insert("service.name".into(), "checkout".into());
    event.metric = Some(MetricPoint {
        name: "http.server.duration".into(),
        value: 12.5,
        stale: false,
        unit: Some("ms".into()),
        temporality: MetricTemporality::Delta,
        exemplars: Vec::new(),
    });
    event
}

fn span() -> EventEnvelope {
    let mut event = EventEnvelope::for_project(
        "project-a",
        "prod",
        "span-1",
        SignalKind::Span,
        serde_json::json!({
            "name": "POST /checkout",
            "start_time_unix_nano": 10,
            "end_time_unix_nano": 2_000_010,
            "status": {"code": "error"}
        }),
    );
    event.trace_id = Some("trace-1".into());
    event.span_id = Some("root".into());
    event
        .resource
        .insert("service.name".into(), "checkout".into());
    event
}

async fn post_json(
    app: &axum::Router,
    uri: &str,
    value: serde_json::Value,
) -> axum::response::Response {
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

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn one_query_endpoint_serves_logs_metrics_and_traces() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    state
        .journal()
        .append(log("log-1", "payment failed"))
        .unwrap();
    state
        .journal()
        .append(log("log-2", "retry failed"))
        .unwrap();
    state.journal().append(metric()).unwrap();
    state.journal().append(span()).unwrap();
    let app = router(state);

    let logs = post_json(
        &app,
        "/api/v1/query",
        serde_json::json!({
            "version": 1,
            "project": "project-a",
            "environment": "prod",
            "signal": {
                "kind": "logs",
                "filter": {
                    "op": "and",
                    "args": [
                        {"op": "eq", "field": "severity", "value": "ERROR"},
                        {"op": "text", "field": "body_text", "value": "payment failed"}
                    ]
                }
            },
            "limit": 10,
            "mode": "sync"
        }),
    )
    .await;
    assert_eq!(logs.status(), StatusCode::OK);
    let logs = body_json(logs).await;
    assert_eq!(logs["data"]["records"].as_array().unwrap().len(), 1);
    assert_eq!(logs["data"]["records"][0]["event_id"], "log-1");
    assert!(logs["watermark"].as_u64().unwrap() >= 4);
    assert_eq!(logs["partial"], false);
    assert!(logs["warnings"].as_array().unwrap().is_empty());
    assert_eq!(logs["stats"]["returned"], 1);
    assert!(logs.get("next_cursor").is_some());
    assert!(logs.get("query_id").is_some());

    let metrics = post_json(
        &app,
        "/api/v1/query",
        serde_json::json!({
            "version": 1,
            "project": "project-a",
            "environment": "prod",
            "signal": {
                "kind": "metrics",
                "name": "http.server.duration",
                "function": "sum",
                "step_seconds": 60,
                "group_by": ["service.name"]
            }
        }),
    )
    .await;
    assert_eq!(metrics.status(), StatusCode::OK);
    let metrics = body_json(metrics).await;
    assert_eq!(metrics["data"]["series"].as_array().unwrap().len(), 1);
    assert_eq!(metrics["data"]["series"][0]["aggregate"], 12.5);

    let traces = post_json(
        &app,
        "/api/v1/query",
        serde_json::json!({
            "version": 1,
            "project": "project-a",
            "environment": "prod",
            "signal": {
                "kind": "traces",
                "service": "checkout",
                "operation": "POST /checkout",
                "status": "error",
                "min_duration_ms": 1
            }
        }),
    )
    .await;
    assert_eq!(traces.status(), StatusCode::OK);
    let traces = body_json(traces).await;
    assert_eq!(traces["data"]["traces"].as_array().unwrap().len(), 1);
    assert_eq!(traces["data"]["traces"][0]["trace_id"], "trace-1");

    let trace = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces/trace-1?project=project-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(trace.status(), StatusCode::OK);
    assert_eq!(body_json(trace).await["trace_id"], "trace-1");
}

#[tokio::test]
async fn query_rejects_unknown_versions_and_invalid_regex() {
    let temp = tempfile::tempdir().unwrap();
    let app = router(Arc::new(ServiceState::open(temp.path()).unwrap()));

    let version = post_json(
        &app,
        "/api/v1/query",
        serde_json::json!({
            "version": 2,
            "project": "project-a",
            "signal": {"kind": "logs"}
        }),
    )
    .await;
    assert_eq!(version.status(), StatusCode::BAD_REQUEST);

    let regex = post_json(
        &app,
        "/api/v1/query",
        serde_json::json!({
            "version": 1,
            "project": "project-a",
            "signal": {
                "kind": "logs",
                "filter": {"op": "regex", "field": "body_text", "pattern": "["}
            }
        }),
    )
    .await;
    assert_eq!(regex.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn v2_hot_history_survives_without_archive_and_cold_history_stays_retryable() {
    use chrono::{Duration, SecondsFormat, Utc};
    for (age, expected_status) in [
        (Duration::minutes(20), StatusCode::OK),
        (Duration::days(31), StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let state = Arc::new(ServiceState::open(temp.path()).unwrap());
        let end = Utc::now() - age;
        for index in 0..3 {
            let time = end - Duration::seconds(2 - index);
            let mut point = metric();
            point.event_id = format!("history-{index}");
            point.occurred_at = time.to_rfc3339_opts(SecondsFormat::Nanos, true);
            point.observed_at.clone_from(&point.occurred_at);
            let metric = point.metric.as_mut().unwrap();
            metric.name = "pilot_counter".into();
            metric.temporality = MetricTemporality::Cumulative;
            metric.value = 10.0 + index as f64;
            state.journal().append(point).unwrap();
        }
        for project in ["project-a", "project-b"] {
            let mut event = log(project, "historical error");
            event.project = project.into();
            event.occurred_at = end.to_rfc3339_opts(SecondsFormat::Nanos, true);
            event.observed_at.clone_from(&event.occurred_at);
            state.journal().append(event).unwrap();
        }
        let app = router(state);
        let end_text = end.to_rfc3339_opts(SecondsFormat::Nanos, true);
        for signal in [
            serde_json::json!({"kind":"metrics", "expression":"sum(rate(pilot_counter[2s]))"}),
            serde_json::json!({"kind":"logs_count"}),
        ] {
            let metrics = signal["kind"] == "metrics";
            let range = if metrics {
                serde_json::json!({"end":end_text})
            } else {
                serde_json::json!({
                    "start":(end - Duration::seconds(1)).to_rfc3339_opts(SecondsFormat::Nanos,true),
                    "end":(end + Duration::seconds(1)).to_rfc3339_opts(SecondsFormat::Nanos,true)})
            };
            let response = post_json(
                &app,
                "/api/v2/query",
                serde_json::json!({
                "version":2,"project":"project-a","environment":"prod", "time_range":range,
                "signal":signal}),
            )
            .await;
            assert_eq!(
                response.status(),
                expected_status,
                "age={age}, metrics={metrics}"
            );
            let body = body_json(response).await;
            if expected_status == StatusCode::SERVICE_UNAVAILABLE {
                assert_eq!(body["error"], "retention_checkpoint_pending");
                assert_eq!(body["retryable"], true);
            } else if metrics {
                assert_eq!(body["partial"], false);
                assert_eq!(body["data"]["result"].as_array().unwrap().len(), 1);
                let rate = body["data"]["result"][0]["value"][1]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap();
                assert!((rate - 1.0).abs() < 1e-10, "{body}");
            } else {
                assert_eq!(body["partial"], false);
                assert_eq!(body["data"]["count"], 1);
                assert_eq!(body["data"]["exact"], true);
            }
        }
    }
}
