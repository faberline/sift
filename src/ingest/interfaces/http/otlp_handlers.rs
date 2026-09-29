//! The OTLP/HTTP logs, traces and metrics endpoints, and the project header
//! they admit a request under.

use std::sync::Arc;

use anyhow::Result;
use axum::body::{Body, Bytes};
use axum::extract::{Extension, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use service_auth::RoleMapPrincipal;

use crate::access::interfaces::http::project_authorization::authorize_project;
use crate::{retention_rejection, ApiError, ServiceState};

#[utoipa::path(post, path = "/v1/logs", responses((status = 200, description = "OTLP logs export response")))]
pub(crate) async fn ingest_logs(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    ingest_otlp(
        state,
        principal,
        headers,
        body,
        transport_otlp::OtlpSignal::Logs,
    )
    .await
}

#[utoipa::path(post, path = "/v1/traces", responses((status = 200, description = "OTLP traces export response")))]
pub(crate) async fn ingest_traces(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    ingest_otlp(
        state,
        principal,
        headers,
        body,
        transport_otlp::OtlpSignal::Traces,
    )
    .await
}

#[utoipa::path(post, path = "/v1/metrics", responses((status = 200, description = "OTLP metrics export response")))]
pub(crate) async fn ingest_metrics(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    ingest_otlp(
        state,
        principal,
        headers,
        body,
        transport_otlp::OtlpSignal::Metrics,
    )
    .await
}

async fn ingest_otlp(
    state: Arc<ServiceState>,
    principal: Option<Extension<RoleMapPrincipal>>,
    headers: HeaderMap,
    body: Bytes,
    signal: transport_otlp::OtlpSignal,
) -> Result<Response, ApiError> {
    let project = project_header(&headers)?;
    authorize_project(principal.as_ref().map(|value| &value.0), project)?;
    let media = transport_otlp::OtlpMediaType::parse(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
    )
    .map_err(|error| ApiError::bad_request("unsupported_content_type", error.to_string()))?;
    let decoded_body = state
        .admission
        .decode_body(&headers, body)
        .map_err(ApiError::from_admission)?;
    state
        .ensure_local_capacity(decoded_body.len())
        .map_err(ApiError::from_admission)?;
    let decoded =
        crate::ingest::interfaces::otlp::otlp_codec::decode(signal, media, &decoded_body, project)
            .map_err(|error| ApiError::bad_request("invalid_otlp", error.to_string()))?;
    let _permit = state
        .admission
        .acquire(project, decoded.item_count(), state.is_draining())
        .map_err(ApiError::from_admission)?;
    let mut rejected = 0usize;
    let mut messages = Vec::new();
    let mut accepted = Vec::new();
    for item in decoded.items {
        let event = match item {
            Ok(event) => event,
            Err(error) => {
                rejected += 1;
                if messages.len() < 8 {
                    messages.push(error.message);
                }
                continue;
            }
        };
        if event.project != project {
            rejected += 1;
            if messages.len() < 8 {
                messages.push(format!(
                    "event project `{}` does not match admitted project `{project}`",
                    event.project
                ));
            }
            continue;
        }
        let event_bytes = serde_json::to_vec(&event)
            .map(|value| value.len())
            .unwrap_or(usize::MAX);
        if let Err(error) = state.admission.validate_event_bytes(event_bytes) {
            rejected += 1;
            if messages.len() < 8 {
                messages.push(error.message);
            }
            continue;
        }
        if let Some(message) = retention_rejection(&event) {
            rejected += 1;
            if messages.len() < 8 {
                messages.push(message);
            }
            continue;
        }
        accepted.push(event);
    }
    let accepted_count = accepted.len();
    if let Err(error) = state.append_events(accepted).await {
        rejected += accepted_count;
        if messages.len() < 8 {
            messages.push(format!("durable batch append failed: {error}"));
        }
    }
    let encoded = crate::ingest::interfaces::otlp::otlp_codec::encode_response(
        signal, media, rejected, &messages,
    )
    .map_err(|error| ApiError::internal(error.to_string()))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, encoded.content_type)
        .body(Body::from(encoded.body))
        .map_err(|error| ApiError::internal(error.to_string()))
}

fn project_header(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get("x-sift-project")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ApiError::bad_request(
                "missing_project",
                "x-sift-project is required for bounded ingest",
            )
        })
}
