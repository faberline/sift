//! Records have revisions. A batch either changes all records or none.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    AlertRule,
    Alert,
    ErrorGroup,
    SavedQuery,
    Notification,
    Cursor,
    Recovery,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRecord {
    pub project: String,
    pub kind: RecordKind,
    pub id: String,
    pub revision: u64,
    pub value: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Change {
    pub project: String,
    pub kind: RecordKind,
    pub id: String,
    pub expected_revision: u64,
    pub value: Option<Value>,
}

impl Change {
    pub fn key(&self) -> String {
        serde_json::to_string(&(&self.project, self.kind, &self.id))
            .expect("control key serialization")
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.project.trim().is_empty()
            || self.project.len() > 512
            || self.id.is_empty()
            || self.id.len() > 128
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
        {
            return Err("control project or record ID is invalid".into());
        }
        if self.value.as_ref().is_some_and(|value| {
            serde_json::to_vec(value).map_or(true, |bytes| bytes.len() > 1024 * 1024)
        }) {
            return Err("control record exceeds 1 MiB".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlCommand {
    pub request_id: String,
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct CommitReceipt {
    pub index: u64,
    pub command_sha256: String,
    pub applied: bool,
    pub message: Option<String>,
}
