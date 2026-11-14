//! Control writes need project admin rights, and one project's admin cannot touch another project.

use std::{collections::HashMap, sync::Arc};

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use service_auth::{Role, TokenClaims};
use sift::{
    access::{SiftAuthConfig, SiftVerifier},
    protected_router, ServiceState,
};
use tower::ServiceExt;

fn claims(subject: &str, project: &str, role: Role) -> TokenClaims {
    TokenClaims::new(
        subject.to_string(),
        HashMap::from([(project.to_string(), role)]),
    )
}

fn app() -> (tempfile::TempDir, Router) {
    let dir = tempfile::tempdir().expect("temporary journal");
    let state = Arc::new(ServiceState::open(dir.path()).expect("open journal"));
    let verifier = Arc::new(SiftVerifier::new(SiftAuthConfig {
        required: true,
        tokens: HashMap::from([
            (
                "admin-a".to_string(),
                claims("admin-a", "project-a", Role::Admin),
            ),
            (
                "reader-a".to_string(),
                claims("reader-a", "project-a", Role::Read),
            ),
            (
                "collector-a".to_string(),
                claims("collector-a", "project-a", Role::Write),
            ),
            (
                "admin-b".to_string(),
                claims("admin-b", "project-b", Role::Admin),
            ),
        ]),
    }));
    (dir, protected_router(state, verifier))
}

/// Sends one request. `token` is `None` for a request without an `authorization` header.
async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    header_project: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-sift-project", header_project);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = match body {
        Some(value) => {
            request = request.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    header_project: &str,
    body: Option<Value>,
) -> StatusCode {
    call(app, method, uri, Some(token), header_project, body)
        .await
        .0
}

fn saved_query(project: &str, request_id: &str) -> Value {
    json!({
        "project": project,
        "request_id": request_id,
        "expected_revision": 0,
        "value": {"request": {"version": 2, "project": project,
            "signal": {"kind": "metrics", "expression": "sum(requests_total)"}}}
    })
}

fn alert_rule(project: &str, request_id: &str) -> Value {
    json!({
        "project": project,
        "request_id": request_id,
        "expected_revision": 0,
        "value": {"version": 1, "project": project, "environment": "prod",
            "source": {"kind": "metrics", "expression": "sum(requests_total)",
                "comparison": "greater", "threshold": 10.0},
            "webhook": "validation"}
    })
}

/// The six management writes, each with its body for `project`.
fn management_writes(project: &str) -> Vec<(&'static str, &'static str, Value)> {
    vec![
        ("PUT", "/api/v1/alert-rules/r", alert_rule(project, "w-1")),
        (
            "DELETE",
            "/api/v1/alert-rules/r",
            json!({"project": project, "request_id": "w-2", "expected_revision": 1}),
        ),
        (
            "PUT",
            "/api/v1/saved-queries/q",
            saved_query(project, "w-3"),
        ),
        (
            "DELETE",
            "/api/v1/saved-queries/q",
            json!({"project": project, "request_id": "w-4", "expected_revision": 1}),
        ),
        (
            "PUT",
            "/api/v1/alerts/r/mute",
            json!({"project": project, "request_id": "w-5", "expected_revision": 0, "until": null}),
        ),
        (
            "PUT",
            "/api/v1/error-groups/g",
            json!({"project": project, "request_id": "w-6", "expected_revision": 0, "status": "resolved"}),
        ),
    ]
}

#[tokio::test]
async fn admin_of_a_project_can_create_a_rule_and_a_saved_query() {
    let (_dir, app) = app();

    let rule = send(
        &app,
        "PUT",
        "/api/v1/alert-rules/r",
        "admin-a",
        "project-a",
        Some(alert_rule("project-a", "c-1")),
    )
    .await;
    assert_eq!(rule, StatusCode::OK);
    let saved = send(
        &app,
        "PUT",
        "/api/v1/saved-queries/q",
        "admin-a",
        "project-a",
        Some(saved_query("project-a", "c-2")),
    )
    .await;
    assert_eq!(saved, StatusCode::OK);

    let (status, body) = call(
        &app,
        "GET",
        "/api/v1/alert-rules?project=project-a",
        Some("admin-a"),
        "project-a",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let records = body["records"].as_array().expect("records array");
    assert_eq!(records.len(), 1, "body: {body}");
    assert_eq!(records[0]["id"], "r");
}

#[tokio::test]
async fn reader_gets_403_on_every_management_write() {
    let (_dir, app) = app();

    for (method, uri, body) in management_writes("project-a") {
        let status = send(&app, method, uri, "reader-a", "project-a", Some(body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
    }

    let (status, _) = call(
        &app,
        "GET",
        "/api/v1/alert-rules?project=project-a",
        Some("reader-a"),
        "project-a",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "a reader still reads");

    let (status, body) = call(
        &app,
        "GET",
        "/api/v1/alert-rules?project=project-a",
        Some("admin-a"),
        "project-a",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["records"].as_array().expect("records array").len(),
        0,
        "denied writes created nothing"
    );
}

#[tokio::test]
async fn collector_credential_gets_403_on_every_management_route() {
    let (_dir, app) = app();

    for (method, uri, body) in management_writes("project-a") {
        let status = send(&app, method, uri, "collector-a", "project-a", Some(body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
    }
}

#[tokio::test]
async fn admin_of_another_project_cannot_read_or_change_the_rule() {
    let (_dir, app) = app();

    let created = send(
        &app,
        "PUT",
        "/api/v1/alert-rules/r",
        "admin-a",
        "project-a",
        Some(alert_rule("project-a", "b-0")),
    )
    .await;
    assert_eq!(created, StatusCode::OK);
    let (_, before) = call(
        &app,
        "GET",
        "/api/v1/alert-rules?project=project-a",
        Some("admin-a"),
        "project-a",
        None,
    )
    .await;
    let original = before["records"][0].clone();
    assert_eq!(original["id"], "r");

    // Body project is project-a while the header names project-b: layer 2 refuses.
    let body_only = [
        (
            "PUT",
            "/api/v1/alert-rules/r",
            alert_rule("project-a", "b-1"),
        ),
        (
            "DELETE",
            "/api/v1/alert-rules/r",
            json!({"project": "project-a", "request_id": "b-2", "expected_revision": 1}),
        ),
        (
            "PUT",
            "/api/v1/saved-queries/q",
            saved_query("project-a", "b-3"),
        ),
        (
            "PUT",
            "/api/v1/alerts/r/mute",
            json!({"project": "project-a", "request_id": "b-4", "expected_revision": 0, "until": null}),
        ),
        (
            "PUT",
            "/api/v1/error-groups/g",
            json!({"project": "project-a", "request_id": "b-5", "expected_revision": 0, "status": "resolved"}),
        ),
    ];
    for (method, uri, body) in body_only {
        let status = send(&app, method, uri, "admin-b", "project-b", Some(body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "layer 2: {method} {uri}");
    }

    // Header names project-a: layer 1 refuses admin-b, who has no role there.
    let status = send(
        &app,
        "PUT",
        "/api/v1/alert-rules/r",
        "admin-b",
        "project-a",
        Some(alert_rule("project-a", "b-6")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "layer 1: PUT rule");

    let (status, _) = call(
        &app,
        "GET",
        "/api/v1/alert-rules?project=project-a",
        Some("admin-b"),
        "project-b",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "GET with project-b header");
    let (status, _) = call(
        &app,
        "GET",
        "/api/v1/alert-rules?project=project-a",
        Some("admin-b"),
        "project-a",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "GET with project-a header");

    let (_, after) = call(
        &app,
        "GET",
        "/api/v1/alert-rules?project=project-a",
        Some("admin-a"),
        "project-a",
        None,
    )
    .await;
    assert_eq!(
        after["records"][0]["revision"], original["revision"],
        "revision unchanged"
    );
    assert_eq!(
        after["records"][0]["value"], original["value"],
        "value unchanged"
    );
}

#[tokio::test]
async fn unknown_and_missing_tokens_are_401() {
    let (_dir, app) = app();

    let (status, _) = call(
        &app,
        "PUT",
        "/api/v1/alert-rules/r",
        Some("nobody"),
        "project-a",
        Some(alert_rule("project-a", "a-1")),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "unknown token");

    let (status, _) = call(
        &app,
        "PUT",
        "/api/v1/alert-rules/r",
        None,
        "project-a",
        Some(alert_rule("project-a", "a-2")),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "no authorization header");
}
