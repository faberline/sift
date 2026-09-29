//! The Prometheus remote-write endpoint.

use std::sync::Arc;

use anyhow::Result;
use axum::body::{Body, Bytes};
use axum::extract::{Extension, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use service_auth::RoleMapPrincipal;

use crate::access::interfaces::http::project_authorization::authorize_project;
use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::ingest::application::retention_admission::retention_rejection;
use crate::ingest::domain::admission_error::AdmissionError;

pub(crate) async fn prometheus_remote_write(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let content_encoding = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let remote_write_version = headers
        .get("x-prometheus-remote-write-version")
        .and_then(|value| value.to_str().ok());
    metrics_remote_write::validate_headers(content_type, content_encoding, remote_write_version)
        .map_err(|error| ApiError::unsupported_media(error.to_string()))?;
    let limits = state.admission.limits();
    if body.len() > limits.max_compressed_body_bytes {
        return Err(ApiError::from_admission(AdmissionError::invalid(
            "compressed_body_too_large",
            "compressed remote write body exceeds the configured limit",
        )));
    }
    let decoded = metrics_remote_write::decode_snappy(&body, limits.max_decoded_body_bytes)
        .map_err(|error| match error {
            metrics_remote_write::DecodeError::BodyTooLarge { .. } => ApiError::bad_request(
                "decoded_body_too_large",
                "decoded remote write body exceeds the configured limit",
            ),
            other => ApiError::bad_request("invalid_snappy", other.to_string()),
        })?;
    state
        .ensure_local_capacity(decoded.len())
        .map_err(ApiError::from_admission)?;
    let admitted_project = headers
        .get("x-sift-project")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty());
    let decoded =
        crate::ingest::interfaces::remote_write::remote_write_consumer::decode_remote_write(
            &decoded,
            admitted_project,
        )
        .map_err(|error| ApiError::bad_request("invalid_remote_write", error.to_string()))?;
    authorize_project(
        principal.as_ref().map(|principal| &principal.0),
        &decoded.project,
    )?;
    let _permit = state
        .admission
        .acquire(&decoded.project, decoded.events.len(), state.is_draining())
        .map_err(ApiError::from_admission)?;
    let written = decoded.events.len();
    for event in &decoded.events {
        let bytes = serde_json::to_vec(&event)
            .map(|bytes| bytes.len())
            .unwrap_or(usize::MAX);
        state
            .admission
            .validate_event_bytes(bytes)
            .map_err(ApiError::from_admission)?;
        if let Some(message) = retention_rejection(event) {
            return Err(ApiError::bad_request("outside_retention", message));
        }
    }
    state
        .append_events(decoded.events)
        .await
        .map_err(|error| ApiError::internal(format!("remote write append failed: {error}")))?;
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header("x-prometheus-remote-write-samples-written", written)
        .body(Body::empty())
        .map_err(|error| ApiError::internal(error.to_string()))
}
