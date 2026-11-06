//! Map gRPC metadata and status while using the common access use case.

use crate::access::application::authorization_service::AuthorizationService;
use crate::access::application::project_authorization::authorize_project;
use axum::http::{HeaderMap, HeaderValue};
use service_auth::AuthError;
use tonic::Status;

pub(crate) async fn authorize(
    metadata: &tonic::metadata::MetadataMap,
    verifier: &AuthorizationService,
) -> Result<String, Status> {
    let project = metadata
        .get("x-sift-project")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Status::invalid_argument("x-sift-project metadata is required"))?
        .to_string();
    let mut headers = HeaderMap::new();
    if let Some(authorization) = metadata
        .get("authorization")
        .and_then(|value| value.to_str().ok())
    {
        headers.insert(
            "authorization",
            HeaderValue::from_str(authorization)
                .map_err(|_| Status::unauthenticated("authorization metadata is invalid"))?,
        );
    }
    let principal = verifier
        .authenticate_write(&headers, &project)
        .await
        .map_err(auth_status)?;
    authorize_project(Some(&principal), &project)
        .map_err(|error| Status::permission_denied(error.message))?;
    Ok(project)
}

fn auth_status(error: AuthError) -> Status {
    match error {
        AuthError::Unauthenticated => Status::unauthenticated("valid bearer token required"),
        AuthError::Forbidden(message) => Status::permission_denied(message),
        AuthError::Unavailable(message) => Status::unavailable(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::infrastructure::{auth_config::SiftAuthConfig, sift_verifier::SiftVerifier};
    use service_auth::{Role, TokenClaims};
    use std::{collections::HashMap, sync::Arc};

    #[tokio::test]
    async fn grpc_project_authorization_keeps_status_and_error_text() {
        let service =
            AuthorizationService::from_verifier(Arc::new(SiftVerifier::new(SiftAuthConfig {
                required: true,
                tokens: HashMap::from([(
                    "writer-token".into(),
                    TokenClaims {
                        subject: "writer".into(),
                        roles: HashMap::from([("project-a".into(), Role::Write)]),
                    },
                )]),
            })));
        for (project, token, code, message) in [
            (
                None,
                None,
                tonic::Code::InvalidArgument,
                "x-sift-project metadata is required",
            ),
            (
                Some("   "),
                None,
                tonic::Code::InvalidArgument,
                "x-sift-project metadata is required",
            ),
            (
                Some("project-a"),
                None,
                tonic::Code::Unauthenticated,
                "valid bearer token required",
            ),
            (
                Some("project-a"),
                Some("Bearer unknown"),
                tonic::Code::Unauthenticated,
                "valid bearer token required",
            ),
            (
                Some("project-b"),
                Some("Bearer writer-token"),
                tonic::Code::PermissionDenied,
                "subject `writer` lacks Write access to project `project-b`",
            ),
        ] {
            let mut metadata = tonic::metadata::MetadataMap::new();
            if let Some(project) = project {
                metadata.insert("x-sift-project", project.parse().unwrap());
            }
            if let Some(token) = token {
                metadata.insert("authorization", token.parse().unwrap());
            }
            let status = authorize(&metadata, &service).await.unwrap_err();
            assert_eq!(status.code(), code);
            assert_eq!(status.message(), message);
        }
        let mut metadata = tonic::metadata::MetadataMap::new();
        metadata.insert("x-sift-project", "project-a".parse().unwrap());
        metadata.insert("authorization", "Bearer writer-token".parse().unwrap());
        assert_eq!(authorize(&metadata, &service).await.unwrap(), "project-a");
    }

    #[tokio::test]
    async fn grpc_open_auth_keeps_project_whitespace() {
        let service = AuthorizationService::from_verifier(Arc::new(SiftVerifier::new(
            SiftAuthConfig::open(),
        )));
        let mut metadata = tonic::metadata::MetadataMap::new();
        metadata.insert("x-sift-project", " project-a ".parse().unwrap());
        assert_eq!(authorize(&metadata, &service).await.unwrap(), " project-a ");
    }
}
