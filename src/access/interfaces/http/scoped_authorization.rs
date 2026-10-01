//! The scoped bearer-token middleware: which role a request needs, for which
//! project, and the MCP transport that authorizes each tool call later.

use axum::http::{HeaderMap, Method, Uri};
use service_auth::{AuthError, RoleMapPrincipal};

use crate::access::application::authorization_service::AuthorizationService;

#[async_trait::async_trait]
impl service_auth::ScopedAuthorization for AuthorizationService {
    type Principal = RoleMapPrincipal;

    async fn authorize_scope(
        &self,
        headers: &HeaderMap,
        method: &Method,
        uri: &Uri,
    ) -> std::result::Result<service_auth::ScopedAuthorizationOutcome<Self::Principal>, AuthError>
    {
        self.authorize_scope(headers, method.as_str(), uri.path())
            .await
    }
}

pub use service_auth::scoped_authorization_middleware as auth_middleware;
