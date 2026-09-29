//! The state machine's control record: its applied index and pending fence.

use serde::{Deserialize, Serialize};

use crate::journal::domain::retention_fence::RetentionFenceV1;

pub(in crate::journal) const CONTROL_STATE_FORMAT_VERSION: u16 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal) struct ControlState {
    pub(in crate::journal) format_version: u16,
    pub(in crate::journal) applied_index: u64,
    #[serde(default)]
    pub(in crate::journal) pending_retention: Option<RetentionFenceV1>,
}

impl Default for ControlState {
    fn default() -> Self {
        Self {
            format_version: CONTROL_STATE_FORMAT_VERSION,
            applied_index: 0,
            pending_retention: None,
        }
    }
}
