//! The shared durable transaction service for rules, errors, and alerts.
use crate::control::domain::port::ControlPort;
pub(crate) use crate::shared_kernel::control_record::{Change, CommitReceipt, ControlCommand};
pub use crate::shared_kernel::control_record::{ControlRecord, RecordKind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

pub struct ControlService {
    port: Arc<dyn ControlPort>,
    pending_notifications: AtomicU64,
    notification_failures: AtomicU64,
    leader: AtomicBool,
}
impl ControlService {
    pub(crate) fn new(port: Arc<dyn ControlPort>) -> Self {
        Self {
            port,
            pending_notifications: AtomicU64::new(0),
            notification_failures: AtomicU64::new(0),
            leader: AtomicBool::new(false),
        }
    }
    pub(crate) async fn transact(
        &self,
        request_id: String,
        changes: Vec<Change>,
    ) -> Result<CommitReceipt, String> {
        let receipt = self
            .port
            .commit(ControlCommand {
                request_id,
                changes,
            })
            .await?;
        if !receipt.applied {
            return Err(receipt
                .message
                .unwrap_or_else(|| "control transaction failed".into()));
        }
        Ok(receipt)
    }
    pub async fn list(
        &self,
        project: &str,
        kind: RecordKind,
    ) -> Result<Vec<ControlRecord>, String> {
        if project.trim().is_empty() {
            return Err("project is required".into());
        }
        self.port.records(Some(project), kind).await
    }
    pub(crate) async fn all(&self, kind: RecordKind) -> Result<Vec<ControlRecord>, String> {
        self.port.records(None, kind).await
    }
    pub async fn is_leader(&self) -> Result<bool, String> {
        let result = self.port.leader().await;
        self.leader.store(
            result.as_ref().is_ok_and(|leader| *leader),
            Ordering::Relaxed,
        );
        result
    }
    pub(crate) fn notification_metrics(&self) -> (u64, u64, bool) {
        (
            self.pending_notifications.load(Ordering::Relaxed),
            self.notification_failures.load(Ordering::Relaxed),
            self.leader.load(Ordering::Relaxed),
        )
    }
    pub(crate) fn record_notification_failure(&self) {
        self.notification_failures.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn set_pending_notifications(&self, count: u64) {
        self.pending_notifications.store(count, Ordering::Relaxed);
    }
    pub fn snapshot_bytes(&self) -> Result<Vec<u8>, String> {
        self.port.snapshot()
    }
}
