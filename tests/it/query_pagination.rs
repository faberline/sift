use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use sift::{router, EventEnvelope, MetricPoint, MetricTemporality, ServiceState, SignalKind};
use tower::ServiceExt;

const PAGE_LIMIT: u64 = 2;
const MAX_PAGES: usize = 10;

fn log(project: &str, id: &str) -> EventEnvelope {
    let mut event = EventEnvelope::for_project(
        project,
        "prod",
        id,
        SignalKind::Log,
        serde_json::json!({"message": id}),
    );
    event
        .resource
        .insert("service.name".into(), "checkout".into());
    event
}

fn metric(project: &str, name: &str) -> EventEnvelope {
    let mut event = EventEnvelope::for_project(
        project,
        "prod",
        format!("event-{name}"),
        SignalKind::Metric,
        serde_json::json!({}),
    );
    event
        .resource
        .insert("service.name".into(), "checkout".into());
    event.metric = Some(MetricPoint {
        name: name.into(),
        value: 1.0,
        stale: false,
        unit: None,
        temporality: MetricTemporality::Delta,
        exemplars: Vec::new(),
    });
    event
}

fn span(project: &str, trace_id: &str) -> EventEnvelope {
    let mut event = EventEnvelope::for_project(
        project,
        "prod",
        format!("span-{trace_id}"),
        SignalKind::Span,
        serde_json::json!({
            "name": "POST /checkout",
            "start_time_unix_nano": 10,
            "end_time_unix_nano": 2_000_010,
            "status": {"code": "error"}
        }),
    );
    event.trace_id = Some(trace_id.into());
    event.span_id = Some("root".into());
    event
        .resource
        .insert("service.name".into(), "checkout".into());
    event
}

fn store(state: &ServiceState, events: impl IntoIterator<Item = EventEnvelope>) {
    for event in events {
        state.journal().append(event).unwrap();
    }
}

fn logs() -> serde_json::Value {
    serde_json::json!({"kind": "logs"})
}

fn metrics() -> serde_json::Value {
    serde_json::json!({"kind": "metrics", "function": "raw"})
}

fn traces() -> serde_json::Value {
    serde_json::json!({"kind": "traces"})
}

fn request(
    project: &str,
    signal: serde_json::Value,
    cursor: Option<&str>,
    limit: u64,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "version": 1,
        "project": project,
        "environment": "prod",
        "signal": signal,
        "limit": limit,
        "mode": "sync"
    });
    if let Some(cursor) = cursor {
        value["cursor"] = serde_json::json!(cursor);
    }
    value
}

async fn post(
    app: &axum::Router,
    project: &str,
    signal: serde_json::Value,
    cursor: Option<&str>,
    limit: u64,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/query")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&request(project, signal, cursor, limit)).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn page(
    app: &axum::Router,
    project: &str,
    signal: serde_json::Value,
    cursor: Option<&str>,
) -> serde_json::Value {
    let response = post(app, project, signal, cursor, PAGE_LIMIT).await;
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await
}

async fn page_with_limit(
    app: &axum::Router,
    project: &str,
    signal: serde_json::Value,
    limit: u64,
) -> serde_json::Value {
    let response = post(app, project, signal, None, limit).await;
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await
}

// Reads every page after `cursor` (None means from the first page).
async fn follow(
    app: &axum::Router,
    project: &str,
    signal: serde_json::Value,
    mut cursor: Option<String>,
) -> Vec<serde_json::Value> {
    let mut pages = Vec::new();
    loop {
        let current = page(app, project, signal.clone(), cursor.as_deref()).await;
        cursor = next(&current);
        pages.push(current);
        if cursor.is_none() {
            return pages;
        }
        assert!(pages.len() < MAX_PAGES, "paging did not finish: {pages:?}");
    }
}

fn next(page: &serde_json::Value) -> Option<String> {
    page["next_cursor"].as_str().map(str::to_owned)
}

fn sizes(pages: &[serde_json::Value], list: &str) -> Vec<usize> {
    pages
        .iter()
        .map(|page| page["data"][list].as_array().expect("page list").len())
        .collect()
}

fn values(pages: &[serde_json::Value], list: &str, key: &str) -> Vec<String> {
    pages
        .iter()
        .flat_map(|page| {
            page["data"][list]
                .as_array()
                .expect("page list")
                .iter()
                .map(|item| item[key].as_str().expect("string key").to_owned())
        })
        .collect()
}

fn ids(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

fn names(prefix: &str, range: std::ops::RangeInclusive<u32>) -> Vec<String> {
    range.map(|i| format!("{prefix}{i}")).collect()
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn logs_pages_hold_each_record_once_in_order() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    store(
        &state,
        names("log-", 1..=5).iter().map(|id| log("project-a", id)),
    );
    let app = router(state.clone());

    let pages = follow(&app, "project-a", logs(), None).await;
    assert_eq!(sizes(&pages, "records"), vec![2, 2, 1]);
    assert_eq!(
        ids(&values(&pages, "records", "event_id")),
        ids(&names("log-", 1..=5))
    );
    let has_cursor: Vec<bool> = pages.iter().map(|p| next(p).is_some()).collect();
    assert_eq!(has_cursor, vec![true, true, false]);
}

#[tokio::test]
async fn logs_append_after_page_one_does_not_repeat_or_skip() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    store(
        &state,
        names("log-", 1..=5).iter().map(|id| log("project-a", id)),
    );
    let app = router(state.clone());

    let first = page(&app, "project-a", logs(), None).await;
    store(&state, [log("project-a", "log-6")]);
    let mut pages = vec![first.clone()];
    pages.extend(follow(&app, "project-a", logs(), next(&first)).await);
    assert_eq!(
        ids(&values(&pages, "records", "event_id")),
        ids(&names("log-", 1..=6))
    );
}

#[tokio::test]
async fn metrics_pages_hold_each_series_once_in_order() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    store(
        &state,
        names("metric.", 1..=5)
            .iter()
            .map(|name| metric("project-a", name)),
    );
    let app = router(state.clone());

    let pages = follow(&app, "project-a", metrics(), None).await;
    assert_eq!(sizes(&pages, "series"), vec![2, 2, 1]);
    let mut seen = values(&pages, "series", "name");
    seen.sort();
    assert_eq!(ids(&seen), ids(&names("metric.", 1..=5)));
    let series_ids = values(&pages, "series", "series_id");
    assert!(series_ids.windows(2).all(|w| w[0] < w[1]), "{series_ids:?}");
}

#[tokio::test]
async fn metrics_append_after_page_one_does_not_repeat_or_skip() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    store(
        &state,
        names("metric.", 1..=5)
            .iter()
            .map(|name| metric("project-a", name)),
    );
    let app = router(state.clone());

    let first = page(&app, "project-a", metrics(), None).await;
    store(&state, [metric("project-a", "metric.6")]);
    let mut pages = vec![first.clone()];
    pages.extend(follow(&app, "project-a", metrics(), next(&first)).await);

    let seen = values(&pages, "series", "name");
    for name in names("metric.", 1..=5) {
        assert_eq!(seen.iter().filter(|n| **n == name).count(), 1, "{name}");
    }
    assert!(
        seen.iter().filter(|n| *n == "metric.6").count() <= 1,
        "{seen:?}"
    );
    let series_ids = values(&pages, "series", "series_id");
    assert!(series_ids.windows(2).all(|w| w[0] < w[1]), "{series_ids:?}");
}

#[tokio::test]
async fn traces_pages_hold_each_trace_once_in_order() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    store(
        &state,
        names("trace-", 1..=5)
            .iter()
            .map(|id| span("project-a", id)),
    );
    let app = router(state.clone());

    let pages = follow(&app, "project-a", traces(), None).await;
    assert_eq!(sizes(&pages, "traces"), vec![2, 2, 1]);
    assert_eq!(
        ids(&values(&pages, "traces", "trace_id")),
        ids(&names("trace-", 1..=5))
    );
}

#[tokio::test]
async fn traces_append_after_page_one_does_not_repeat_or_skip() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    store(
        &state,
        names("trace-", 1..=5)
            .iter()
            .map(|id| span("project-a", id)),
    );
    let app = router(state.clone());

    let first = page(&app, "project-a", traces(), None).await;
    store(&state, [span("project-a", "trace-6")]);
    let mut pages = vec![first.clone()];
    pages.extend(follow(&app, "project-a", traces(), next(&first)).await);
    assert_eq!(
        ids(&values(&pages, "traces", "trace_id")),
        ids(&names("trace-", 1..=6))
    );
}

#[tokio::test]
async fn a_cursor_of_another_signal_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    store(
        &state,
        names("log-", 1..=3).iter().map(|id| log("project-a", id)),
    );
    store(
        &state,
        names("metric.", 1..=2)
            .iter()
            .map(|name| metric("project-a", name)),
    );
    store(
        &state,
        names("trace-", 1..=2)
            .iter()
            .map(|id| span("project-a", id)),
    );
    let app = router(state.clone());

    let log_cursor = next(&page(&app, "project-a", logs(), None).await).expect("log cursor");
    let metric_cursor =
        next(&page_with_limit(&app, "project-a", metrics(), 1).await).expect("metric cursor");
    let trace_cursor =
        next(&page_with_limit(&app, "project-a", traces(), 1).await).expect("trace cursor");

    let cases = [
        ("metrics", metrics(), log_cursor.as_str()),
        ("traces", traces(), log_cursor.as_str()),
        ("logs", logs(), metric_cursor.as_str()),
        ("logs", logs(), trace_cursor.as_str()),
        ("logs", logs(), "garbage"),
    ];
    for (signal, value, cursor) in cases {
        let response = post(&app, "project-a", value, Some(cursor), PAGE_LIMIT).await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "{signal} {cursor}"
        );
        let body = body_json(response).await;
        assert_eq!(body["error"], "invalid_cursor", "{signal} {cursor}");
    }
}

#[tokio::test]
async fn a_cursor_from_another_project_never_shows_that_projects_records() {
    let temp = tempfile::tempdir().unwrap();
    let state = Arc::new(ServiceState::open(temp.path()).unwrap());
    store(
        &state,
        names("log-", 1..=5).iter().map(|id| log("project-a", id)),
    );
    store(&state, [log("project-b", "b-1")]);
    let app = router(state.clone());

    let cursor = next(&page(&app, "project-a", logs(), None).await).expect("cursor");
    let other = page(&app, "project-b", logs(), Some(&cursor)).await;
    let other_ids = values(std::slice::from_ref(&other), "records", "event_id");
    assert_eq!(ids(&other_ids), vec!["b-1"]);
}
