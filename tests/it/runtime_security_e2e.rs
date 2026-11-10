// HANDWRITE-BEGIN gap="sift-auth-route-contract-tests" tracker="1604" reason="Verify required bearer auth protects data-plane routes while probes remain reachable."
use std::{collections::HashMap, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use service_auth::{Role, TokenClaims};
use sift::{
    access::{SiftAuthConfig, SiftVerifier},
    openapi, protected_router, ServiceState,
};
use tower::ServiceExt;

fn required_app(data_dir: &std::path::Path) -> axum::Router {
    let verifier = Arc::new(SiftVerifier::new(SiftAuthConfig {
        required: true,
        tokens: HashMap::from([(
            "writer-token".to_string(),
            TokenClaims {
                subject: "sift-test".to_string(),
                roles: HashMap::from([("*".to_string(), Role::Write)]),
            },
        )]),
    }));
    let state = Arc::new(ServiceState::open(data_dir).expect("open journal"));
    service_http::standard_probe_routes(state.clone(), Some(state.clone()), openapi)
        .merge(protected_router(state, verifier))
}

fn valid_logs() -> serde_json::Value {
    serde_json::json!({
        "resourceLogs": [{
            "resource": {"attributes": [
                {"key": "service.name", "value": {"stringValue": "sift-test"}}
            ]},
            "scopeLogs": [{"logRecords": [{"body": {"stringValue": "ok"}}]}]
        }]
    })
}

#[tokio::test]
async fn required_auth_protects_data_plane_but_not_operational_probes() {
    let data_dir = tempfile::tempdir().expect("temporary journal");
    let app = required_app(data_dir.path());

    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/logs")
                .header("content-type", "application/json")
                .header("x-sift-project", "project-a")
                .body(Body::from(serde_json::to_vec(&valid_logs()).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let accepted = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/logs")
                .header("content-type", "application/json")
                .header("x-sift-project", "project-a")
                .header("authorization", "Bearer writer-token")
                .body(Body::from(serde_json::to_vec(&valid_logs()).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
}

async fn get(app: &axum::Router, uri: &str) -> axum::response::Response {
    app.clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn post_logs(
    app: &axum::Router,
    project: &str,
    bearer: Option<&str>,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/logs")
        .header("content-type", "application/json")
        .header("x-sift-project", project);
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    app.clone()
        .oneshot(
            builder
                .body(Body::from(serde_json::to_vec(&valid_logs()).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn required_app_with_roles_and_state(
    data_dir: &std::path::Path,
) -> (axum::Router, Arc<sift::ServiceState>) {
    let verifier = Arc::new(SiftVerifier::new(SiftAuthConfig {
        required: true,
        tokens: HashMap::from([
            (
                "writer-token".to_string(),
                TokenClaims {
                    subject: "sift-writer".into(),
                    roles: HashMap::from([("project-a".to_string(), Role::Write)]),
                },
            ),
            (
                "reader-token".to_string(),
                TokenClaims {
                    subject: "sift-reader".into(),
                    roles: HashMap::from([("project-a".to_string(), Role::Read)]),
                },
            ),
        ]),
    }));
    let state = Arc::new(ServiceState::open(data_dir).expect("open journal"));
    let app = service_http::standard_probe_routes(state.clone(), Some(state.clone()), openapi)
        .merge(protected_router(state.clone(), verifier));
    (app, state)
}

fn required_app_with_roles(data_dir: &std::path::Path) -> axum::Router {
    required_app_with_roles_and_state(data_dir).0
}

#[tokio::test]
async fn all_operational_probes_are_reachable_without_a_bearer_token() {
    let data_dir = tempfile::tempdir().expect("temporary journal");
    let app = required_app(data_dir.path());

    for uri in ["/healthz", "/readyz", "/metrics", "/openapi.json", "/docs"] {
        let response = get(&app, uri).await;
        assert_eq!(response.status(), StatusCode::OK, "probe {uri}");
        let body = body_text(response).await;
        assert!(!body.is_empty(), "probe {uri} returned an empty body");
    }

    let openapi_body = body_text(get(&app, "/openapi.json").await).await;
    let openapi: serde_json::Value =
        serde_json::from_str(&openapi_body).expect("/openapi.json is JSON");
    assert!(
        openapi.get("openapi").is_some(),
        "missing top-level openapi key"
    );
}

#[tokio::test]
async fn a_wrong_bearer_token_is_rejected_with_401() {
    let data_dir = tempfile::tempdir().expect("temporary journal");
    let app = required_app(data_dir.path());

    let response = post_logs(&app, "project-a", Some("not-a-token")).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_read_only_token_cannot_write() {
    let data_dir = tempfile::tempdir().expect("temporary journal");
    let app = required_app_with_roles(data_dir.path());

    let reader = post_logs(&app, "project-a", Some("reader-token")).await;
    assert_eq!(reader.status(), StatusCode::FORBIDDEN);

    let writer = post_logs(&app, "project-a", Some("writer-token")).await;
    assert_eq!(writer.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_token_is_bound_to_its_project() {
    let data_dir = tempfile::tempdir().expect("temporary journal");
    let app = required_app_with_roles(data_dir.path());

    let response = post_logs(&app, "project-b", Some("writer-token")).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn rejected_responses_never_echo_a_secret() {
    let data_dir = tempfile::tempdir().expect("temporary journal");
    let app = required_app_with_roles(data_dir.path());

    let rejected = [
        post_logs(&app, "project-a", None).await,
        post_logs(&app, "project-a", Some("not-a-token")).await,
        post_logs(&app, "project-a", Some("reader-token")).await,
    ];
    for response in rejected {
        let status = response.status();
        assert!(
            status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN,
            "expected 401 or 403, got {status}"
        );
        let body = body_text(response).await;
        assert!(!body.contains("reader-token"), "body echoes reader-token");
        assert!(!body.contains("writer-token"), "body echoes writer-token");
        assert!(!body.contains("not-a-token"), "body echoes the wrong token");
    }
}

#[tokio::test]
async fn rejected_writes_store_nothing() {
    let data_dir = tempfile::tempdir().expect("temporary journal");
    let (app, state) = required_app_with_roles_and_state(data_dir.path());

    let no_token = post_logs(&app, "project-a", None).await;
    assert_eq!(no_token.status(), StatusCode::UNAUTHORIZED);
    let reader = post_logs(&app, "project-a", Some("reader-token")).await;
    assert_eq!(reader.status(), StatusCode::FORBIDDEN);
    assert_eq!(state.journal().query(Default::default()).unwrap().len(), 0);

    let writer = post_logs(&app, "project-a", Some("writer-token")).await;
    assert_eq!(writer.status(), StatusCode::OK);
    assert_eq!(state.journal().query(Default::default()).unwrap().len(), 1);
}

// HANDWRITE-END
