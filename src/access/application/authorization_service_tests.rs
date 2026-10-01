//! Check the route decisions and exact credentials passed through the port.

use super::authorization_service::AuthorizationService;
use crate::access::domain::{project_verifier::ProjectVerifier, route_role::RouteRole};
use crate::shared_kernel::port_future::PortFuture;
use axum::http::HeaderMap;
use service_auth::{AuthError, RoleMapPrincipal, ScopedAuthorizationOutcome};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct VerifierProbe {
    calls: Mutex<Vec<(String, RouteRole)>>,
}

impl ProjectVerifier for VerifierProbe {
    type Credentials = HeaderMap;
    type Principal = RoleMapPrincipal;
    type Error = AuthError;
    fn is_kubernetes(&self) -> bool {
        true
    }
    fn authenticate_project<'a>(
        &'a self,
        _headers: &'a HeaderMap,
        project: &'a str,
        role: RouteRole,
    ) -> PortFuture<'a, RoleMapPrincipal, AuthError> {
        self.calls.lock().unwrap().push((project.into(), role));
        Box::pin(async { Ok(RoleMapPrincipal::Open) })
    }
}

#[tokio::test]
async fn request_roles_project_trim_and_mcp_bypass_reach_the_shared_port() {
    let probe = Arc::new(VerifierProbe::default());
    let service = AuthorizationService::new(probe.clone());
    let mut headers = HeaderMap::new();
    headers.insert("x-sift-project", " project-a ".parse().unwrap());
    let routes = [
        ("GET", "/api/v1/services", RouteRole::Read),
        ("POST", "/api/v1/query", RouteRole::Read),
        ("POST", "/v1/logs", RouteRole::Write),
        ("POST", "/admin/integrity", RouteRole::Admin),
    ];
    for (method, path, _) in routes {
        assert!(matches!(
            service
                .authorize_scope(&headers, method, path)
                .await
                .unwrap(),
            ScopedAuthorizationOutcome::Authorized(_)
        ));
    }
    assert_eq!(
        *probe.calls.lock().unwrap(),
        routes.map(|(_, _, role)| ("project-a".into(), role))
    );
    assert!(matches!(
        service
            .authorize_scope(&HeaderMap::new(), "POST", "/mcp")
            .await
            .unwrap(),
        ScopedAuthorizationOutcome::Bypass
    ));
    let error = service
        .authorize_scope(&HeaderMap::new(), "GET", "/api/v1/services")
        .await
        .unwrap_err();
    assert!(matches!(error, AuthError::Forbidden(message) if message ==
        "x-sift-project is required for Kubernetes project authorization"));
    assert_eq!(probe.calls.lock().unwrap().len(), routes.len());
}
