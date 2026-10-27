//! The shared credential and route authorization use case.

use crate::access::domain::project_verifier::ProjectVerifier;
use crate::access::domain::route_role::{http_role, RouteRole};
use axum::http::HeaderMap;
use service_auth::{AuthError, RoleMapPrincipal, ScopedAuthorizationOutcome};
use std::sync::Arc;

pub struct AuthorizationService {
    verifier: Arc<
        dyn ProjectVerifier<
            Credentials = HeaderMap,
            Principal = RoleMapPrincipal,
            Error = AuthError,
        >,
    >,
}

impl AuthorizationService {
    pub(crate) fn new(
        verifier: Arc<
            dyn ProjectVerifier<
                Credentials = HeaderMap,
                Principal = RoleMapPrincipal,
                Error = AuthError,
            >,
        >,
    ) -> Self {
        Self { verifier }
    }

    fn is_kubernetes(&self) -> bool {
        self.verifier.is_kubernetes()
    }

    async fn authenticate_project(
        &self,
        headers: &HeaderMap,
        project: &str,
        role: RouteRole,
    ) -> Result<RoleMapPrincipal, AuthError> {
        self.verifier
            .authenticate_project(headers, project, role)
            .await
    }

    pub(crate) async fn authenticate_write(
        &self,
        headers: &HeaderMap,
        project: &str,
    ) -> Result<RoleMapPrincipal, AuthError> {
        self.authenticate_project(headers, project, RouteRole::Write)
            .await
    }

    pub(crate) async fn authorize_scope(
        &self,
        headers: &HeaderMap,
        method: &str,
        path: &str,
    ) -> Result<ScopedAuthorizationOutcome<RoleMapPrincipal>, AuthError> {
        if self.is_kubernetes() && path == "/mcp" {
            // MCP calls are authorized again when each tool calls the normal API
            // with its explicit project. The outer transport owns Host/Origin.
            return Ok(service_auth::ScopedAuthorizationOutcome::Bypass);
        }
        let project = headers
            .get("x-sift-project")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        if self.is_kubernetes() && project.is_empty() {
            return Err(AuthError::Forbidden(
                "x-sift-project is required for Kubernetes project authorization".into(),
            ));
        }
        let role = http_role(method, path);
        let principal = self.authenticate_project(headers, project, role).await?;
        Ok(service_auth::ScopedAuthorizationOutcome::Authorized(
            principal,
        ))
    }
}
