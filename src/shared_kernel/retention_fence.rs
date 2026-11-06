//! The retention fence a replicated command carries.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RetentionFenceV1 {
    pub source_manifest_uri: String,
    pub source_manifest_sha256: String,
    pub target_generation: u64,
    pub evaluate_at: String,
}
