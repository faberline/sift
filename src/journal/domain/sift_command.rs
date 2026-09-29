//! The replicated Sift command: appending events at a decision time, and
//! clearing a retention fence.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::journal::domain::retention_fence::RetentionFenceV1;
use crate::EventEnvelope;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SiftCommandV1 {
    AppendEvents {
        /// One leader-selected decision time makes duplicate classification
        /// identical on every voter. Legacy commands omit this field and use
        /// a deterministic event-time fallback.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        acknowledged_at: Option<String>,
        events: Vec<EventEnvelope>,
    },
    ArchiveCheckpointBarrier {
        retention_generation: u64,
        manifest_uri: String,
        manifest_sha256: String,
    },
    RetentionFence {
        fence: RetentionFenceV1,
    },
    ClearRetentionFence {
        retention_generation: u64,
    },
}

impl SiftCommandV1 {
    pub(crate) fn append_events_at(
        events: Vec<EventEnvelope>,
        acknowledged_at: DateTime<Utc>,
    ) -> Self {
        Self::AppendEvents {
            acknowledged_at: Some(acknowledged_at.to_rfc3339_opts(SecondsFormat::Nanos, true)),
            events,
        }
    }

    pub(crate) fn append_events_now(events: Vec<EventEnvelope>) -> Self {
        Self::append_events_at(events, Utc::now())
    }

    pub(crate) fn append_events_size_bound(events: Vec<EventEnvelope>) -> Self {
        Self::AppendEvents {
            acknowledged_at: Some("9999-12-31T23:59:59.999999999Z".to_string()),
            events,
        }
    }

    pub(crate) fn clear_retention_fence(retention_generation: u64) -> Self {
        Self::ClearRetentionFence {
            retention_generation,
        }
    }
}
