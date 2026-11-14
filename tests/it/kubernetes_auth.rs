//! GKE authentication delegates identity and project policy to Kubernetes.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use service_auth::k8s::{
    AccessReviewOutcome, ResourceAttributes, ReviewBackend, ReviewError, ReviewedIdentity,
    TokenReviewOutcome,
};
use sift::{access::SiftVerifier, protected_router, ServiceState};
use tower::ServiceExt;

struct ReviewRecorder {
    allow: bool,
    accesses: Mutex<Vec<ResourceAttributes>>,
}

#[async_trait]
impl ReviewBackend for ReviewRecorder {
    async fn review_token(
        &self,
        _token: &str,
        audiences: &[String],
    ) -> Result<TokenReviewOutcome, ReviewError> {
        Ok(TokenReviewOutcome::new(
            true,
            ReviewedIdentity {
                username: "system:serviceaccount:caller:agent".into(),
                uid: "caller-agent".into(),
                groups: vec!["system:serviceaccounts".into()],
                extra: Default::default(),
            },
            audiences.to_vec(),
            None,
        ))
    }

    async fn review_access(
        &self,
        _identity: &ReviewedIdentity,
        attributes: &ResourceAttributes,
    ) -> Result<AccessReviewOutcome, ReviewError> {
        self.accesses.lock().unwrap().push(attributes.clone());
        Ok(if self.allow {
            AccessReviewOutcome::allow()
        } else {
            AccessReviewOutcome::deny("test policy")
        })
    }
}

fn query(project: &str) -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "project": project,
        "environment": "prod",
        "signal": {"kind": "logs"},
        "limit": 10,
        "mode": "sync"
    })
}

async fn request(app: axum::Router, header_project: &str, body_project: &str) -> StatusCode {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri("/api/v1/query")
            .header("content-type", "application/json")
            .header("authorization", "Bearer projected-service-account-token")
            .header("x-sift-project", header_project)
            .body(Body::from(
                serde_json::to_vec(&query(body_project)).unwrap(),
            ))
            .unwrap(),
    )
    .await
    .unwrap()
    .status()
}

#[tokio::test]
async fn token_review_and_subject_access_review_bound_each_request_to_one_project() {
    let backend = Arc::new(ReviewRecorder {
        allow: true,
        accesses: Mutex::new(Vec::new()),
    });
    let verifier = Arc::new(
        SiftVerifier::kubernetes(backend.clone(), "sift.axiom.dev", "observability")
            .expect("build delegated verifier"),
    );
    let data = tempfile::tempdir().unwrap();
    let app = protected_router(Arc::new(ServiceState::open(data.path()).unwrap()), verifier);

    assert_eq!(
        request(app.clone(), "project-a", "project-a").await,
        StatusCode::OK
    );
    assert_eq!(
        request(app, "project-a", "project-b").await,
        StatusCode::FORBIDDEN
    );

    let accesses = backend.accesses.lock().unwrap();
    assert_eq!(accesses.len(), 1, "the cached identical review is reused");
    assert_eq!(accesses[0].group, "sift.axiom.dev");
    assert_eq!(accesses[0].namespace, "observability");
    assert_eq!(accesses[0].resource, "projects");
    assert_eq!(accesses[0].name.as_deref(), Some("project-a"));
    assert_eq!(accesses[0].verb, "get");
}

#[tokio::test]
async fn a_subject_access_review_deny_is_a_forbidden_response() {
    let backend = Arc::new(ReviewRecorder {
        allow: false,
        accesses: Mutex::new(Vec::new()),
    });
    let verifier = Arc::new(
        SiftVerifier::kubernetes(backend, "sift.axiom.dev", "observability")
            .expect("build delegated verifier"),
    );
    let data = tempfile::tempdir().unwrap();
    let app = protected_router(Arc::new(ServiceState::open(data.path()).unwrap()), verifier);

    assert_eq!(
        request(app, "project-a", "project-a").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn global_backup_requires_a_namespace_admin_review() {
    let backend = Arc::new(ReviewRecorder {
        allow: true,
        accesses: Mutex::new(Vec::new()),
    });
    let verifier = Arc::new(
        SiftVerifier::kubernetes(backend.clone(), "sift.axiom.dev", "observability")
            .expect("build delegated verifier"),
    );
    let data = tempfile::tempdir().unwrap();
    let app = protected_router(Arc::new(ServiceState::open(data.path()).unwrap()), verifier);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/admin/backup")
                .header("authorization", "Bearer projected-service-account-token")
                .header("x-sift-project", "project-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let accesses = backend.accesses.lock().unwrap();
    assert_eq!(accesses[0].name.as_deref(), None);
    assert_eq!(accesses[0].verb, "update");
}

#[tokio::test]
async fn project_admin_cannot_write_control_records_in_another_project() {
    let backend = Arc::new(ReviewRecorder {
        allow: true,
        accesses: Mutex::new(Vec::new()),
    });
    let verifier = Arc::new(
        SiftVerifier::kubernetes(backend.clone(), "sift.axiom.dev", "observability").unwrap(),
    );
    let dir = tempfile::tempdir().unwrap();
    let app = protected_router(Arc::new(ServiceState::open(dir.path()).unwrap()), verifier);
    for (project, expected) in [
        ("project-a", StatusCode::OK),
        ("project-b", StatusCode::FORBIDDEN),
    ] {
        let value = serde_json::json!({"project":project,"request_id":format!("write-{project}"),"expected_revision":0,
            "value":{"request":{"version":2,"project":project,"signal":{"kind":"metrics","expression":"up"}}}});
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/saved-queries/q")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer projected-service-account-token")
                    .header("x-sift-project", "project-a")
                    .body(Body::from(serde_json::to_vec(&value).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let reviews = backend.accesses.lock().unwrap();
    assert!(reviews
        .iter()
        .all(|review| review.name.as_deref() == Some("project-a")));
    assert_eq!(reviews[0].verb, "update");
}
