//! The verifier: checks a bearer token against the static role map, or asks the
//! Kubernetes API server to authenticate it and review its project access.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use axum::http::HeaderMap;
use service_auth::k8s::{
    DelegatedAuthConfig, DelegatedAuthenticator, KubeReviewBackend, ResourceAttributes,
    ReviewBackend,
};
use service_auth::{
    AuthError, Role, RoleMapPrincipal, StaticRoleMapVerifier, TokenClaims, Verifier,
};

use crate::access::domain::route_role::role_verb;
use crate::access::infrastructure::auth_config::SiftAuthConfig;

#[derive(Clone)]
enum SiftVerifierInner {
    Static(StaticRoleMapVerifier),
    Kubernetes {
        authenticator: Arc<DelegatedAuthenticator>,
        namespace: Arc<str>,
    },
}

#[derive(Clone)]
pub struct SiftVerifier(SiftVerifierInner);

impl SiftVerifier {
    pub fn new(config: SiftAuthConfig) -> Self {
        Self(SiftVerifierInner::Static(StaticRoleMapVerifier::new(
            config.required,
            config.tokens,
        )))
    }

    pub fn kubernetes(
        backend: Arc<dyn ReviewBackend>,
        audience: &str,
        namespace: &str,
    ) -> Result<Self> {
        let audience = audience.trim();
        let namespace = namespace.trim();
        if audience.is_empty() {
            bail!("Sift Kubernetes audience must not be empty");
        }
        if namespace.is_empty() {
            bail!("Sift serving namespace must not be empty");
        }
        let config = DelegatedAuthConfig::new(vec![audience.to_string()])
            .context("build Sift Kubernetes delegated-auth configuration")?;
        Ok(Self(SiftVerifierInner::Kubernetes {
            authenticator: Arc::new(DelegatedAuthenticator::new(backend, config)),
            namespace: Arc::from(namespace),
        }))
    }

    pub async fn from_env() -> Result<Self> {
        if std::env::var("SIFT_AUTH")
            .ok()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("kubernetes"))
        {
            let audience =
                std::env::var("SIFT_K8S_AUDIENCE").unwrap_or_else(|_| "sift.axiom.dev".to_string());
            let namespace = std::env::var("POD_NAMESPACE")
                .context("POD_NAMESPACE is required when SIFT_AUTH=kubernetes")?;
            let config = DelegatedAuthConfig::new(vec![audience.trim().to_string()])
                .context("build Sift Kubernetes delegated-auth configuration")?;
            let backend = KubeReviewBackend::in_cluster()
                .await
                .map_err(|error| anyhow::anyhow!("initialize Kubernetes reviews: {error}"))?;
            let probe = ResourceAttributes::new(
                "sift.axiom.dev",
                namespace.trim(),
                "projects",
                Some("sift-delegation-probe".to_string()),
                "get",
            );
            backend
                .probe_delegation(config.audiences(), &probe)
                .await
                .map_err(|error| anyhow::anyhow!("probe Kubernetes auth delegation: {error}"))?;
            return Ok(Self(SiftVerifierInner::Kubernetes {
                authenticator: Arc::new(DelegatedAuthenticator::new(Arc::new(backend), config)),
                namespace: Arc::from(namespace.trim()),
            }));
        }
        Ok(Self::new(SiftAuthConfig::from_env()?))
    }

    pub fn required(&self) -> bool {
        match &self.0 {
            SiftVerifierInner::Static(verifier) => verifier.required(),
            SiftVerifierInner::Kubernetes { .. } => true,
        }
    }

    pub fn is_kubernetes(&self) -> bool {
        matches!(&self.0, SiftVerifierInner::Kubernetes { .. })
    }

    pub async fn authenticate_project(
        &self,
        headers: &HeaderMap,
        project: &str,
        role: Role,
    ) -> std::result::Result<RoleMapPrincipal, AuthError> {
        match &self.0 {
            SiftVerifierInner::Static(verifier) => verifier.authenticate(headers),
            SiftVerifierInner::Kubernetes {
                authenticator,
                namespace,
            } => {
                let token =
                    service_auth::bearer_token(headers).ok_or(AuthError::Unauthenticated)?;
                let caller = authenticator
                    .authenticate(token)
                    .await
                    .map_err(AuthError::from)?;
                let attributes = ResourceAttributes::new(
                    "sift.axiom.dev",
                    namespace.as_ref(),
                    "projects",
                    Some(project.to_string()),
                    role_verb(role),
                );
                authenticator
                    .authorize(&caller, &attributes)
                    .await
                    .map_err(AuthError::from)?;
                Ok(RoleMapPrincipal::Token(TokenClaims {
                    subject: caller.username(),
                    roles: HashMap::from([(
                        if role == Role::Admin {
                            "*".to_string()
                        } else {
                            project.to_string()
                        },
                        role,
                    )]),
                }))
            }
        }
    }
}
