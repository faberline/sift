//! The direct OTLP/gRPC ingest server: project authorization from gRPC
//! metadata, and each export admitted, decoded and appended like OTLP/HTTP.

use std::future::Future;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue};
use service_auth::{AuthError, Role};
use tonic::Status;

use crate::access::infrastructure::sift_verifier::SiftVerifier;
use crate::access::interfaces::http::project_authorization::authorize_project;
use crate::ingest::interfaces::otlp::otlp_codec::normalize_payload;
use crate::ServiceState;

#[derive(Clone)]
struct SiftGrpcConsumer {
    state: Arc<ServiceState>,
}

#[derive(Clone)]
struct SiftGrpcAuthorizer {
    verifier: Arc<SiftVerifier>,
}

pub async fn serve<F>(
    listener: tokio::net::TcpListener,
    state: Arc<ServiceState>,
    verifier: Arc<SiftVerifier>,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let maximum = state.admission.limits().max_decoded_body_bytes;
    transport_otlp::serve_grpc(
        listener,
        Arc::new(SiftGrpcConsumer { state }),
        Arc::new(SiftGrpcAuthorizer { verifier }),
        maximum,
        shutdown,
    )
    .await
}

#[tonic::async_trait]
impl transport_otlp::GrpcProjectAuthorizer for SiftGrpcAuthorizer {
    async fn authorize(&self, metadata: &tonic::metadata::MetadataMap) -> Result<String, Status> {
        authorize(metadata, &self.verifier).await
    }
}

#[tonic::async_trait]
impl transport_otlp::OtlpConsumer for SiftGrpcConsumer {
    async fn consume(
        &self,
        project: &str,
        payload: transport_otlp::DecodedPayload,
    ) -> transport_otlp::Result<transport_otlp::PartialSuccess> {
        let decoded = normalize_payload(payload, project).map_err(|error| {
            transport_otlp::TransportError::Consumer {
                message: error.to_string(),
            }
        })?;
        let _permit = self
            .state
            .admission
            .acquire(project, decoded.item_count(), self.state.is_draining())
            .map_err(|error| transport_otlp::TransportError::Consumer {
                message: error.message,
            })?;
        let mut rejected = 0usize;
        let mut messages = Vec::new();
        let mut accepted = Vec::new();
        let mut accepted_bytes = 0usize;
        for item in decoded.items {
            let event = match item {
                Ok(event) => event,
                Err(error) => {
                    rejected += 1;
                    push_message(&mut messages, error.message);
                    continue;
                }
            };
            if event.project != project {
                rejected += 1;
                push_message(
                    &mut messages,
                    format!(
                        "event project `{}` does not match admitted project `{project}`",
                        event.project
                    ),
                );
                continue;
            }
            let event_bytes = serde_json::to_vec(&event)
                .map(|bytes| bytes.len())
                .unwrap_or(usize::MAX);
            if let Err(error) = self.state.admission.validate_event_bytes(event_bytes) {
                rejected += 1;
                push_message(&mut messages, error.message);
                continue;
            }
            if let Some(message) = crate::retention_rejection(&event) {
                rejected += 1;
                push_message(&mut messages, message);
                continue;
            }
            accepted_bytes = accepted_bytes.saturating_add(event_bytes);
            accepted.push(event);
        }
        self.state
            .ensure_local_capacity(accepted_bytes)
            .map_err(|error| transport_otlp::TransportError::Consumer {
                message: format!("{}; retry after 5 seconds", error.message),
            })?;
        let accepted_count = accepted.len();
        if let Err(error) = self.state.append_events(accepted).await {
            rejected += accepted_count;
            push_message(
                &mut messages,
                format!("durable batch append failed: {error}"),
            );
        }
        Ok(transport_otlp::PartialSuccess::new(
            rejected,
            messages.join("; "),
        ))
    }
}

pub(crate) async fn authorize(
    metadata: &tonic::metadata::MetadataMap,
    verifier: &SiftVerifier,
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
        .authenticate_project(&headers, &project, Role::Write)
        .await
        .map_err(auth_status)?;
    authorize_project(Some(&principal), &project)
        .map_err(|error| Status::permission_denied(error.message))?;
    Ok(project)
}

fn push_message(messages: &mut Vec<String>, message: String) {
    if messages.len() < 8 {
        messages.push(message);
    }
}

fn auth_status(error: AuthError) -> Status {
    match error {
        AuthError::Unauthenticated => Status::unauthenticated("valid bearer token required"),
        AuthError::Forbidden(message) => Status::permission_denied(message),
        AuthError::Unavailable(message) => Status::unavailable(message),
    }
}
