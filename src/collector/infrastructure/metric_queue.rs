//! Save every batch from a scrape before sending its first batch.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Deserialize, Serialize)]
pub(super) struct MetricQueue {
    pub project: String,
    pub environment: String,
    pub node: String,
    pub batches: Vec<Value>,
    pub next: usize,
}
impl MetricQueue {
    pub fn load(path: &Path, project: &str, environment: &str, node: &str) -> Result<Option<Self>> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
            bail!("pending metric queue must be a regular file of at most 64 MiB");
        }
        let bytes = std::fs::read(path)?;
        if bytes.is_empty() {
            return Ok(None);
        }
        if bytes.len() > 64 * 1024 * 1024 {
            bail!("pending metric queue exceeds 64 MiB");
        }
        let queue: Self = serde_json::from_slice(&bytes)?;
        if queue.project != project
            || queue.environment != environment
            || queue.node != node
            || queue.next > queue.batches.len()
        {
            bail!("pending metric queue identity or offset is invalid");
        }
        Ok(Some(queue))
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() > 64 * 1024 * 1024 {
            bail!("pending metric queue exceeds 64 MiB");
        }
        storage_durable::atomic_write(path, &bytes, storage_durable::FsyncPolicy::Always)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restart_retains_the_entire_scrape_and_acked_position() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.json");
        let mut q = MetricQueue {
            project: "p".into(),
            environment: "e".into(),
            node: "n".into(),
            batches: vec![serde_json::json!({"time":1}), serde_json::json!({"time":2})],
            next: 0,
        };
        q.save(&path).unwrap();
        q.next = 1;
        q.save(&path).unwrap();
        let restored = MetricQueue::load(&path, "p", "e", "n").unwrap().unwrap();
        assert_eq!(restored.next, 1);
        assert_eq!(restored.batches, q.batches);
        assert!(MetricQueue::load(&path, "other", "e", "n").is_err());
    }
}
