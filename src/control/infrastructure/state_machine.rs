//! A private, atomic state file and a versioned Raft snapshot.
use crate::control::domain::{
    records::{CommitReceipt, ControlCommand, ControlRecord, RecordKind},
    state::{ControlState, MAX_STATE_BYTES},
};
use crate::shared_kernel::sha256_hex::sha256_hex;
use anyhow::{bail, Context, Result};
use raft_runtime::RaftStateMachine;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

const MAGIC: &[u8] = b"SIFT-CONTROL-1\n";
#[cfg(test)]
mod tests;
pub(crate) struct ControlStateMachine {
    path: PathBuf,
    state: Mutex<ControlState>,
}
impl ControlStateMachine {
    pub(crate) fn validate_bytes(snapshot: &[u8]) -> Result<()> {
        Self::decode_snapshot(&mut std::io::Cursor::new(snapshot)).map(|_| ())
    }
    pub(crate) fn bootstrap(root: &Path, snapshot: &[u8]) -> Result<()> {
        let mut restored = Self::decode_snapshot(&mut std::io::Cursor::new(snapshot))?;
        let path = root.join("control/product-state-v1.json");
        if path.exists() {
            bail!("refuse to overwrite existing control state");
        }
        // A fresh Raft group starts at zero. Product revisions and notification
        // IDs survive. Receipts with indices from the old Raft group do not.
        restored.applied_index = 0;
        restored.receipts.clear();
        storage_durable::atomic_write(
            &path,
            &serde_json::to_vec(&restored)?,
            storage_durable::FsyncPolicy::Always,
        )
    }
    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join("control/product-state-v1.json");
        let state = if path.exists() {
            let bytes = std::fs::read(&path)?;
            if bytes.len() > MAX_STATE_BYTES {
                bail!("control state exceeds 64 MiB");
            }
            let state: ControlState = serde_json::from_slice(&bytes)?;
            state.validate().map_err(anyhow::Error::msg)?;
            state
        } else {
            ControlState::default()
        };
        Ok(Self {
            path,
            state: Mutex::new(state),
        })
    }
    pub fn records(&self, project: Option<&str>, kind: RecordKind) -> Vec<ControlRecord> {
        self.state
            .lock()
            .expect("control state lock")
            .records
            .values()
            .filter(|r| project.is_none_or(|p| r.project == p) && r.kind == kind)
            .cloned()
            .collect()
    }
    pub fn receipt(&self, id: &str) -> Option<CommitReceipt> {
        self.state
            .lock()
            .expect("control state lock")
            .receipts
            .get(id)
            .cloned()
    }
    pub fn bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.snapshot(&mut bytes)?;
        Ok(bytes)
    }
    fn decode_snapshot(reader: &mut dyn Read) -> Result<ControlState> {
        let mut bytes = Vec::new();
        reader
            .take((MAX_STATE_BYTES + MAGIC.len() + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_STATE_BYTES + MAGIC.len() {
            bail!("control snapshot exceeds 64 MiB");
        }
        let payload = bytes
            .strip_prefix(MAGIC)
            .context("invalid control snapshot header")?;
        let state: ControlState = serde_json::from_slice(payload)?;
        state.validate().map_err(anyhow::Error::msg)?;
        Ok(state)
    }
}
impl RaftStateMachine for ControlStateMachine {
    fn snapshot_capability(&self) -> Option<&'static str> {
        Some("sift-control-v1")
    }
    fn applied_index(&self) -> u64 {
        self.state.lock().expect("control state lock").applied_index
    }
    fn apply(&self, index: u64, bytes: &[u8]) -> Result<()> {
        if bytes.len() > 4 * 1024 * 1024 {
            bail!("control command exceeds 4 MiB");
        }
        let command: ControlCommand = serde_json::from_slice(bytes)?;
        let mut state = self.state.lock().expect("control state lock");
        if index <= state.applied_index {
            return Ok(());
        }
        let next = state
            .apply(index, &command, sha256_hex(bytes))
            .map_err(anyhow::Error::msg)?;
        storage_durable::atomic_write(
            &self.path,
            &serde_json::to_vec(&next)?,
            storage_durable::FsyncPolicy::Always,
        )?;
        *state = next;
        Ok(())
    }
    fn snapshot(&self, writer: &mut dyn Write) -> Result<()> {
        let state = self.state.lock().expect("control state lock");
        writer.write_all(MAGIC)?;
        serde_json::to_writer(writer, &*state)?;
        Ok(())
    }
    fn validate_snapshot(&self, reader: &mut dyn Read) -> Result<()> {
        Self::decode_snapshot(reader).map(|_| ())
    }
    fn snapshot_at(&self, index: u64, writer: &mut dyn Write) -> Result<()> {
        let state = self.state.lock().expect("control state lock");
        if state.applied_index != index {
            bail!("control snapshot index does not match the applied state");
        }
        writer.write_all(MAGIC)?;
        serde_json::to_writer(writer, &*state)?;
        Ok(())
    }
    fn restore(&self, reader: &mut dyn Read) -> Result<()> {
        let restored = Self::decode_snapshot(reader)?;
        let mut state = self.state.lock().expect("control state lock");
        if restored.applied_index < state.applied_index {
            bail!("control snapshot moves applied index backwards");
        }
        storage_durable::atomic_write(
            &self.path,
            &serde_json::to_vec(&restored)?,
            storage_durable::FsyncPolicy::Always,
        )?;
        *state = restored;
        Ok(())
    }
}
