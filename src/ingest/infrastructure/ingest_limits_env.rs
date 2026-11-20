//! Reading the ingest limits from SIFT_* environment variables.

use crate::ingest::domain::ingest_limits::IngestLimits;

impl IngestLimits {
    pub fn from_env() -> anyhow::Result<Self> {
        let defaults = Self::default();
        Ok(Self {
            max_compressed_body_bytes: env_usize(
                "SIFT_MAX_COMPRESSED_BODY_BYTES",
                defaults.max_compressed_body_bytes,
            )?,
            max_decoded_body_bytes: env_usize(
                "SIFT_MAX_DECODED_BODY_BYTES",
                defaults.max_decoded_body_bytes,
            )?,
            max_event_bytes: env_usize("SIFT_MAX_EVENT_BYTES", defaults.max_event_bytes)?,
            max_events_per_batch: env_usize(
                "SIFT_MAX_EVENTS_PER_BATCH",
                defaults.max_events_per_batch,
            )?,
            max_concurrent_requests_per_project: env_usize(
                "SIFT_MAX_CONCURRENT_INGEST_PER_PROJECT",
                defaults.max_concurrent_requests_per_project,
            )?,
            max_items_per_project_window: env_usize(
                "SIFT_MAX_INGEST_ITEMS_PER_PROJECT_WINDOW",
                defaults.max_items_per_project_window,
            )?,
            quota_window_secs: env_u64(
                "SIFT_INGEST_QUOTA_WINDOW_SECS",
                defaults.quota_window_secs,
            )?,
            max_local_storage_bytes: env_u64(
                "SIFT_MAX_LOCAL_STORAGE_BYTES",
                defaults.max_local_storage_bytes,
            )?,
            min_local_free_bytes: env_u64(
                "SIFT_MIN_LOCAL_FREE_BYTES",
                defaults.min_local_free_bytes,
            )?,
            max_inflight_body_bytes: env_usize(
                "SIFT_MAX_INFLIGHT_BODY_BYTES",
                defaults.max_inflight_body_bytes,
            )?,
            max_concurrent_queries: env_usize(
                "SIFT_MAX_CONCURRENT_QUERIES",
                defaults.max_concurrent_queries,
            )?,
        })
    }
}

fn env_usize(name: &str, default: usize) -> anyhow::Result<usize> {
    match std::env::var(name) {
        Ok(value) => Ok(value.parse()?),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

fn env_u64(name: &str, default: u64) -> anyhow::Result<u64> {
    match std::env::var(name) {
        Ok(value) => Ok(value.parse()?),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use crate::ingest::domain::ingest_limits::IngestLimits;

    #[test]
    fn global_limits_are_read_from_the_environment() {
        std::env::set_var("SIFT_MAX_INFLIGHT_BODY_BYTES", "268435456");
        std::env::set_var("SIFT_MAX_CONCURRENT_QUERIES", "7");
        let limits = IngestLimits::from_env().unwrap();
        assert_eq!(limits.max_inflight_body_bytes, 268_435_456);
        assert_eq!(limits.max_concurrent_queries, 7);
        std::env::set_var("SIFT_MAX_CONCURRENT_QUERIES", "0");
        let zero = IngestLimits::from_env().unwrap();
        assert!(zero.validate().is_err());
        std::env::remove_var("SIFT_MAX_INFLIGHT_BODY_BYTES");
        std::env::remove_var("SIFT_MAX_CONCURRENT_QUERIES");
    }
}
