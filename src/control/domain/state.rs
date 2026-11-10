//! Deterministic revision checks, quotas, and command replay.
use super::records::{CommitReceipt, ControlCommand, ControlRecord};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const MAX_STATE_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlState {
    pub format_version: u32,
    pub applied_index: u64,
    pub records: BTreeMap<String, ControlRecord>,
    pub receipts: BTreeMap<String, CommitReceipt>,
}
impl Default for ControlState {
    fn default() -> Self {
        Self {
            format_version: 1,
            applied_index: 0,
            records: BTreeMap::new(),
            receipts: BTreeMap::new(),
        }
    }
}
impl ControlState {
    pub fn apply(
        &self,
        index: u64,
        command: &ControlCommand,
        digest: String,
    ) -> Result<Self, String> {
        let mut next = self.clone();
        next.applied_index = index;
        if let Some(receipt) = self.receipts.get(&command.request_id) {
            // Rejected client commands must not stop the replicated apply loop.
            // The submission adapter checks the digest before returning this receipt.
            let _ = receipt;
            return Ok(next);
        }
        let mut candidate = self.clone();
        let mut keys = BTreeSet::new();
        let mut failure = if command.request_id.is_empty()
            || command.request_id.len() > 256
            || command.changes.len() > 1000
        {
            Some("control request ID or batch size is invalid".into())
        } else {
            None
        };
        for change in &command.changes {
            if let Err(error) = change.validate() {
                failure = Some(error);
                break;
            }
            let key = change.key();
            if !keys.insert(key.clone()) {
                failure = Some("a control transaction changes the same record twice".into());
                break;
            }
            let current = self.records.get(&key).map_or(0, |record| record.revision);
            if current != change.expected_revision {
                failure = Some(format!("revision conflict for {}", change.id));
            }
        }
        if failure.is_none() {
            for change in &command.changes {
                let Some(revision) = change.expected_revision.checked_add(1) else {
                    failure = Some("control revision overflow".into());
                    break;
                };
                candidate.records.insert(
                    change.key(),
                    ControlRecord {
                        project: change.project.clone(),
                        kind: change.kind,
                        id: change.id.clone(),
                        revision,
                        value: change.value.clone(),
                    },
                );
            }
            if candidate.records.len() > 100_000
                || serde_json::to_vec(&candidate.records)
                    .map_err(|e| e.to_string())?
                    .len()
                    > 60 * 1024 * 1024
            {
                failure = Some("control state exceeds its record quota".into());
            }
            if failure.is_none() {
                next.records = candidate.records;
            }
        }
        next.receipts.insert(
            command.request_id.clone(),
            CommitReceipt {
                index,
                command_sha256: digest,
                applied: failure.is_none(),
                message: failure,
            },
        );
        while next.receipts.len() > 4096 {
            let oldest = next
                .receipts
                .iter()
                .min_by_key(|(_, receipt)| receipt.index)
                .map(|(id, _)| id.clone())
                .expect("receipt exists");
            next.receipts.remove(&oldest);
        }
        if serde_json::to_vec(&next).map_err(|e| e.to_string())?.len() > MAX_STATE_BYTES {
            return Err("control state exceeds 64 MiB".into());
        }
        Ok(next)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format_version != 1 || self.records.len() > 100_000 || self.receipts.len() > 4096 {
            return Err("invalid control snapshot version or quota".into());
        }
        for (key, record) in &self.records {
            let change = super::records::Change {
                project: record.project.clone(),
                kind: record.kind,
                id: record.id.clone(),
                expected_revision: record.revision,
                value: record.value.clone(),
            };
            change.validate()?;
            if change.key() != *key || record.revision == 0 {
                return Err("invalid control snapshot record".into());
            }
        }
        for (id, receipt) in &self.receipts {
            if id.len() > 256
                || receipt.index > self.applied_index
                || receipt.command_sha256.len() != 64
                || !receipt
                    .command_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
            {
                return Err("invalid control snapshot receipt".into());
            }
        }
        Ok(())
    }
}
