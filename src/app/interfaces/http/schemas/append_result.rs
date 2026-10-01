//! OpenAPI descriptions for append_result data. Runtime bytes use the original models.

#[allow(dead_code)]
#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct AppendResult {
    pub event_id: String,
    /// Leader-selected Raft decision time. The exact six-hour window starts
    /// at this returned timestamp, not at client-side response receipt time.
    pub acknowledged_at: String,
    /// Compatibility alias for `raw_cursor`.
    pub cursor: u64,
    pub raw_cursor: u64,
    pub commit_index: u64,
    /// True when Sift found the event ID inside its six-hour exact window.
    pub duplicate: bool,
}

schema_adapter!(
    crate::journal::domain::append_result::AppendResult,
    AppendResult
);
