//! The receipt the dedupe index keeps for an acknowledged event.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DedupeReceipt {
    pub project: String,
    pub event_id: String,
    pub cursor: u64,
    pub acknowledged_at_unix_nano: i64,
}
