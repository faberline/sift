//! A complete recovery record binds the data manifest and control snapshot.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DataRecovery {
    pub manifest_uri: String,
    pub manifest_sha256: String,
    pub protected_at: i64,
    pub through_cursor: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRecord {
    pub version: u32,
    pub created_at: i64,
    pub protected_at: i64,
    pub data: DataRecovery,
    pub control_uri: String,
    pub control_sha256: String,
    pub control_index: u64,
}
impl RecoveryRecord {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.protected_at <= 0
            || self.protected_at > self.created_at
            || self.protected_at > self.data.protected_at
            || !self.control_uri.starts_with("gs://")
            || !self.data.manifest_uri.starts_with("gs://")
            || [&self.control_sha256, &self.data.manifest_sha256]
                .iter()
                .any(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("invalid joint recovery record".into());
        }
        Ok(())
    }
}
