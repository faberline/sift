//! Read barriers and durable commands share the control Raft state machine.
use super::state_machine::ControlStateMachine;
use crate::control::domain::{
    port::ControlPort,
    records::{CommitReceipt, ControlCommand, ControlRecord, RecordKind},
};
use crate::shared_kernel::port_future::PortFuture;
use raft_runtime::RaftStateMachine;
use std::sync::Arc;

pub(crate) struct RaftControlPort {
    pub machine: Arc<ControlStateMachine>,
    pub raft: Option<Arc<raft_runtime::RaftHost>>,
    pub local: tokio::sync::Mutex<()>,
    pub gate: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
}
impl RaftControlPort {
    async fn submit(&self, command: &ControlCommand) -> Result<u64, String> {
        if command
            .changes
            .iter()
            .any(|c| c.kind != RecordKind::Recovery)
        {
            (self.gate)()?;
        }
        let bytes = serde_json::to_vec(command).map_err(|e| e.to_string())?;
        if command.request_id.is_empty()
            || command.request_id.len() > 256
            || command.changes.len() > 1000
        {
            return Err("control request ID or batch size is invalid".into());
        }
        if bytes.len() > 4 * 1024 * 1024 {
            return Err("control command exceeds 4 MiB".into());
        }
        if let Some(raft) = &self.raft {
            let index = raft.propose(bytes).await.map_err(|e| e.to_string())?;
            if self.machine.applied_index() < index {
                let mut applied = raft.applied_watch();
                tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    while *applied.borrow_and_update() < index {
                        applied.changed().await.map_err(|e| e.to_string())?;
                    }
                    Ok::<_, String>(())
                })
                .await
                .map_err(|_| "control read-your-write deadline exceeded".to_string())??;
            }
            return Ok(index.get());
        }
        let _guard = self.local.lock().await;
        let index = self
            .machine
            .applied_index()
            .get()
            .checked_add(1)
            .ok_or("control index overflow")?;
        self.machine
            .apply(raft_runtime::Index::new(index), &bytes)
            .map_err(|e| e.to_string())?;
        Ok(index)
    }
}
impl ControlPort for RaftControlPort {
    fn commit(&self, command: ControlCommand) -> PortFuture<'_, CommitReceipt, String> {
        Box::pin(async move {
            self.submit(&command).await?;
            let receipt = self.machine.receipt(&command.request_id).ok_or_else(|| {
                "control receipt is no longer resident; read the record before retry".to_string()
            })?;
            let digest = crate::shared_kernel::sha256_hex::sha256_hex(
                &serde_json::to_vec(&command).map_err(|e| e.to_string())?,
            );
            if receipt.command_sha256 != digest {
                return Err("control request ID was reused for different content".into());
            }
            if command
                .changes
                .iter()
                .any(|c| c.kind != RecordKind::Recovery)
            {
                (self.gate)()?;
            }
            Ok(receipt)
        })
    }
    fn records<'a>(
        &'a self,
        project: Option<&'a str>,
        kind: RecordKind,
    ) -> PortFuture<'a, Vec<ControlRecord>, String> {
        Box::pin(async move {
            if self.raft.is_some() {
                self.submit(&ControlCommand {
                    request_id: format!(
                        "read-{}",
                        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
                    ),
                    changes: Vec::new(),
                })
                .await?;
            }
            Ok(self.machine.records(project, kind))
        })
    }
    fn leader(&self) -> PortFuture<'_, bool, String> {
        Box::pin(async move {
            Ok(match &self.raft {
                Some(raft) => raft.is_leader().await,
                None => true,
            })
        })
    }
    fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.machine.bytes().map_err(|e| e.to_string())
    }
}
