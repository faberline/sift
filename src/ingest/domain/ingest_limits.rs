//! The limits an ingest request is held to, and their validation.

use thiserror::Error;

#[derive(Clone, Debug)]
pub struct IngestLimits {
    pub max_compressed_body_bytes: usize,
    pub max_decoded_body_bytes: usize,
    pub max_event_bytes: usize,
    pub max_events_per_batch: usize,
    pub max_concurrent_requests_per_project: usize,
    pub max_items_per_project_window: usize,
    pub quota_window_secs: u64,
    pub max_local_storage_bytes: u64,
    pub min_local_free_bytes: u64,
}

impl Default for IngestLimits {
    fn default() -> Self {
        Self {
            max_compressed_body_bytes: 1_048_576,
            max_decoded_body_bytes: 8_388_608,
            max_event_bytes: 262_144,
            max_events_per_batch: 1_000,
            max_concurrent_requests_per_project: 32,
            max_items_per_project_window: 720_000,
            quota_window_secs: 60,
            max_local_storage_bytes: 50 * 1024 * 1024 * 1024,
            min_local_free_bytes: 1024 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Error)]
pub enum IngestLimitsError {
    #[error("all ingest limits must be greater than zero")]
    ZeroLimit,
    #[error("compressed body limit must not exceed decoded body limit")]
    CompressedAboveDecoded,
}

impl IngestLimits {
    pub fn validate(&self) -> Result<(), IngestLimitsError> {
        if self.max_compressed_body_bytes == 0
            || self.max_decoded_body_bytes == 0
            || self.max_event_bytes == 0
            || self.max_events_per_batch == 0
            || self.max_concurrent_requests_per_project == 0
            || self.max_items_per_project_window == 0
            || self.quota_window_secs == 0
            || self.max_local_storage_bytes == 0
            || self.min_local_free_bytes == 0
        {
            return Err(IngestLimitsError::ZeroLimit);
        }
        if self.max_compressed_body_bytes > self.max_decoded_body_bytes {
            return Err(IngestLimitsError::CompressedAboveDecoded);
        }
        Ok(())
    }
}
