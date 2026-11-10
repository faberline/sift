//! Product use cases do not own Raft or filesystem types.
use super::records::{CommitReceipt, ControlCommand, ControlRecord, RecordKind};
use crate::shared_kernel::port_future::PortFuture;

pub(crate) trait ControlPort: Send + Sync {
    fn commit(&self, command: ControlCommand) -> PortFuture<'_, CommitReceipt, String>;
    fn records<'a>(
        &'a self,
        project: Option<&'a str>,
        kind: RecordKind,
    ) -> PortFuture<'a, Vec<ControlRecord>, String>;
    fn leader(&self) -> PortFuture<'_, bool, String>;
    fn snapshot(&self) -> Result<Vec<u8>, String>;
}
