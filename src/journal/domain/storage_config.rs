//! How raw storage is configured: blob externalization and segment sealing.

#[derive(Clone, Debug)]
pub struct StorageConfig {
    pub initial_logical_shards: u16,
    pub max_segment_events: usize,
    pub max_segment_bytes: usize,
    pub blob_externalize_bytes: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            initial_logical_shards: 1,
            max_segment_events: 100_000,
            max_segment_bytes: 256 * 1024 * 1024,
            blob_externalize_bytes: 65_536,
        }
    }
}
