//! The scoped bearer-token middleware: which role a request needs, for which
//! project, and the MCP transport that authorizes each tool call later.

use axum::http::{HeaderMap, Method, Uri};
use service_auth::{AuthError, RoleMapPrincipal};

use crate::access::domain::route_role::http_role;
use crate::access::infrastructure::sift_verifier::SiftVerifier;

#[async_trait::async_trait]
impl service_auth::ScopedAuthorization for SiftVerifier {
    type Principal = RoleMapPrincipal;

    async fn authorize_scope(
        &self,
        headers: &HeaderMap,
        method: &Method,
        uri: &Uri,
    ) -> std::result::Result<service_auth::ScopedAuthorizationOutcome<Self::Principal>, AuthError>
    {
        if self.is_kubernetes() && uri.path() == "/mcp" {
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
        let role = http_role(method, uri.path());
        let principal = self.authenticate_project(headers, project, role).await?;
        Ok(service_auth::ScopedAuthorizationOutcome::Authorized(
            principal,
        ))
    }
}

pub use service_auth::scoped_authorization_middleware as auth_middleware;
