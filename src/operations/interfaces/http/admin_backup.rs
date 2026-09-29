//! `GET /admin/backup`: the exact durable-journal snapshot bytes, for a caller
//! with wildcard admin access.

use std::sync::Arc;

use anyhow::Result;
use axum::body::Body;
use axum::extract::{Extension, State};
use axum::http::{header, HeaderValue};
use axum::response::Response;
use service_auth::RoleMapPrincipal;

use crate::access::interfaces::http::project_authorization::authorize_global_admin;
use crate::{ApiError, ServiceState};

#[utoipa::path(
    get,
    path = "/admin/backup",
    responses(
        (status = 200, description = "exact durable-journal snapshot bytes"),
        (status = 403, description = "wildcard admin role required", body = ErrorEnvelope),
        (status = 500, description = "snapshot serialization failed", body = ErrorEnvelope)
    )
)]
pub(crate) async fn admin_backup(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
) -> Result<Response, ApiError> {
    authorize_global_admin(principal.as_ref().map(|principal| &principal.0))?;
    let snapshot = state
        .journal()
        .snapshot_bytes()
        .map_err(|error| ApiError::internal(format!("create durable journal snapshot: {error}")))?;
    tracing::info!(
        event = "backup_started",
        subject = principal
            .as_ref()
            .and_then(|principal| principal.0.subject())
            .unwrap_or("open-auth"),
        bytes = snapshot.len(),
        "durable journal snapshot exported"
    );
    let mut response = Response::new(Body::from(snapshot));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(
            crate::journal::infrastructure::raft::snapshot_format::SNAPSHOT_CONTENT_TYPE,
        ),
    );
    Ok(response)
}
