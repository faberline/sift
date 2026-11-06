//! Pure projection metadata and lag errors.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectionDescriptor {
    pub name: String,
    pub schema_version: u32,
    pub retention: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectionLag {
    pub error: String,
    pub projection: String,
    pub required_cursor: u64,
    pub current_cursor: u64,
    pub retryable: bool,
    pub retry_after_seconds: u64,
}

impl ProjectionLag {
    pub fn new(
        projection: impl Into<String>,
        required_cursor: u64,
        current_cursor: u64,
        retry_after_seconds: u64,
    ) -> Self {
        Self {
            error: "projection_lag".to_string(),
            projection: projection.into(),
            required_cursor,
            current_cursor,
            retryable: true,
            retry_after_seconds,
        }
    }
}

impl std::fmt::Display for ProjectionLag {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "projection {} is at cursor {}, requires {}",
            self.projection, self.current_cursor, self.required_cursor
        )
    }
}

impl std::error::Error for ProjectionLag {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RebuildComparison {
    pub source_cursor: u64,
    pub rebuilt_cursor: u64,
    pub live_digest: String,
    pub rebuilt_digest: String,
    pub equal: bool,
}
