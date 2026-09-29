//! The integrity report's wire format: per-signal counts and watermarks, the
//! event-ID digest, and the WAL and archive state of one project.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::event::SignalKind;
use crate::shared_kernel::stored_event::StoredEvent;

#[derive(Debug, Deserialize)]
pub(crate) struct IntegrityHttpQuery {
    pub(super) project: String,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IntegritySignalV1 {
    pub count: u64,
    pub watermark: u64,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IntegritySignalsV1 {
    pub logs: IntegritySignalV1,
    pub metrics: IntegritySignalV1,
    pub traces: IntegritySignalV1,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IntegrityWatermarksV1 {
    pub logs: u64,
    pub metrics: u64,
    pub traces: u64,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IntegrityWalBytesV1 {
    pub logs: u64,
    pub metrics: u64,
    pub traces: u64,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IntegrityArchiveV1 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed_at: Option<String>,
    pub watermarks: IntegrityWatermarksV1,
    pub retention_generation: u64,
    pub retention_scan_pending: bool,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IntegrityStorageV1 {
    pub wal_bytes: IntegrityWalBytesV1,
    pub archive: IntegrityArchiveV1,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct IntegrityReportV1 {
    pub version: u16,
    pub project: String,
    pub cluster_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_from: Option<String>,
    pub event_count: u64,
    pub event_id_digest_algorithm: String,
    pub event_id_sha256: String,
    pub watermark: u64,
    pub signals: IntegritySignalsV1,
    pub storage: IntegrityStorageV1,
}

impl IntegritySignalsV1 {
    pub(super) fn include(&mut self, event: &StoredEvent) {
        let signal = match event.event.signal {
            SignalKind::Log => &mut self.logs,
            SignalKind::Metric => &mut self.metrics,
            SignalKind::Span => &mut self.traces,
        };
        signal.count = signal.count.saturating_add(1);
        signal.watermark = signal.watermark.max(event.cursor);
    }
}
