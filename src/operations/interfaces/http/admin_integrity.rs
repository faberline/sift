//! `GET /admin/integrity`: scans one project's events and reports their count,
//! event-ID digest and watermarks beside the WAL and archive state.

use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Extension, Query, State};
use axum::Json;
use service_auth::RoleMapPrincipal;
use sha2::{Digest, Sha256};

use crate::access::interfaces::http::project_authorization::authorize_global_admin;
use crate::app::interfaces::http::api_error::ApiError;
use crate::app::service_state::ServiceState;
use crate::node::infrastructure::data_layout::LayoutManifest;
use crate::operations::interfaces::http::integrity_report_v1::{
    IntegrityArchiveV1, IntegrityHttpQuery, IntegrityReportV1, IntegritySignalsV1,
    IntegrityStorageV1, IntegrityWalBytesV1, IntegrityWatermarksV1,
};

#[utoipa::path(
    get,
    path = "/admin/integrity",
    params(("project" = String, Query, description = "project to verify")),
    responses(
        (status = 200, description = "project count, ID digest, and watermarks", body = IntegrityReportV1),
        (status = 403, description = "wildcard admin role required", body = ErrorEnvelope)
    )
)]
pub(crate) async fn admin_integrity(
    State(state): State<Arc<ServiceState>>,
    principal: Option<Extension<RoleMapPrincipal>>,
    Query(query): Query<IntegrityHttpQuery>,
) -> Result<Json<IntegrityReportV1>, ApiError> {
    authorize_global_admin(principal.as_ref().map(|principal| &principal.0))?;
    let project = query.project.trim();
    if project.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_project",
            "integrity project must not be empty",
        ));
    }

    let layout_path = state.journal().storage().root().join("layout.json");
    let layout: LayoutManifest = serde_json::from_slice(
        &std::fs::read(&layout_path)
            .map_err(|error| ApiError::internal(format!("read integrity layout: {error}")))?,
    )
    .map_err(|error| ApiError::internal(format!("decode integrity layout: {error}")))?;
    let storage_root = state.journal().storage().root();
    let archive =
        crate::archive::application::archive_status_queries::committed_status(storage_root)
            .map_err(|error| ApiError::internal(format!("read archive integrity: {error}")))?;
    let watermarks = archive
        .as_ref()
        .map(|status| status.watermarks)
        .unwrap_or_default();
    let wal_bytes = |signal: &str| {
        std::fs::metadata(storage_root.join("wal").join(signal).join("events.framed"))
            .map(|metadata| metadata.len())
            .unwrap_or(0)
    };

    let mut reader = state
        .journal
        .projection_read_session(0)
        .map_err(|error| ApiError::internal(format!("open integrity scan: {error}")))?;
    let mut event_count = 0_u64;
    let mut watermark = 0_u64;
    let mut event_id_digest = [0_u8; 32];
    let mut signals = IntegritySignalsV1::default();
    loop {
        let page = reader
            .read_next(10_000)
            .map_err(|error| ApiError::internal(format!("scan integrity events: {error}")))?;
        if page.is_empty() {
            break;
        }
        for event in page.iter().filter(|event| event.event.project == project) {
            event_count = event_count.saturating_add(1);
            watermark = watermark.max(event.cursor);
            signals.include(event);
            let digest: [u8; 32] = Sha256::digest(event.event.event_id.as_bytes()).into();
            for (slot, byte) in event_id_digest.iter_mut().zip(digest) {
                *slot ^= byte;
            }
        }
    }

    Ok(Json(IntegrityReportV1 {
        version: 1,
        project: project.to_string(),
        cluster_id: layout.cluster_id,
        restored_from: layout.restored_from,
        event_count,
        event_id_digest_algorithm: "xor-sha256-v1".to_string(),
        event_id_sha256: hex::encode(event_id_digest),
        watermark,
        signals,
        storage: IntegrityStorageV1 {
            wal_bytes: IntegrityWalBytesV1 {
                logs: wal_bytes("logs"),
                metrics: wal_bytes("metrics"),
                traces: wal_bytes("traces"),
            },
            archive: IntegrityArchiveV1 {
                manifest_uri: archive.as_ref().map(|status| status.manifest_uri.clone()),
                manifest_sha256: archive
                    .as_ref()
                    .map(|status| status.manifest_sha256.clone()),
                committed_at: archive.as_ref().map(|status| status.committed_at.clone()),
                watermarks: IntegrityWatermarksV1 {
                    logs: watermarks.logs,
                    metrics: watermarks.metrics,
                    traces: watermarks.traces,
                },
                retention_generation: archive
                    .as_ref()
                    .map(|status| status.retention_generation)
                    .unwrap_or_default(),
                retention_scan_pending: archive
                    .as_ref()
                    .is_some_and(|status| status.retention_scan_pending),
            },
        },
    }))
}
