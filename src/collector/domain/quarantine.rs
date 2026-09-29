//! A line the collector rejects, as the quarantine file records it
//! (collector.rejection.v1), with its untrusted text bounded.

use serde::{Deserialize, Serialize};

pub const REJECTION_SCHEMA: &str = "collector.rejection.v1";
pub const MAX_REJECTION_PREVIEW_BYTES: usize = 1024;
pub const MAX_REJECTION_ERROR_BYTES: usize = 512;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct QuarantineEntry {
    pub schema: String,
    pub source_id: String,
    pub line: u64,
    pub offset: u64,
    pub code: String,
    pub message: String,
    pub preview: String,
}

impl QuarantineEntry {
    pub fn invalid_line(
        source_id: &str,
        line: u64,
        offset: u64,
        code: impl Into<String>,
        message: impl AsRef<str>,
        bytes: &[u8],
    ) -> Self {
        Self {
            schema: REJECTION_SCHEMA.to_string(),
            source_id: source_id.to_string(),
            line,
            offset,
            code: code.into(),
            message: truncate_utf8(message.as_ref(), MAX_REJECTION_ERROR_BYTES),
            preview: truncate_utf8(&String::from_utf8_lossy(bytes), MAX_REJECTION_PREVIEW_BYTES),
        }
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}
