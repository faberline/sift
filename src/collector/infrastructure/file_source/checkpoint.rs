//! Persisted file identities and offsets.
use super::super::checkpoint::CollectorCheckpoint;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs::File};
#[derive(Clone, Default, Deserialize, Serialize)]
pub(super) struct Position {
    pub(super) offset: u64,
    pub(super) line: u64,
    pub(super) observed_len: u64,
    #[serde(default)]
    pub(super) retired: bool,
    #[serde(default)]
    pub(super) generation: u64,
}
#[derive(Deserialize, Serialize)]
pub(super) struct FileCheckpoint {
    #[serde(flatten)]
    pub(super) legacy: CollectorCheckpoint,
    #[serde(default)]
    pub(super) files: BTreeMap<String, Position>,
    #[serde(default)]
    pub(super) lost_bytes: u64,
    #[serde(default)]
    pub(super) lost_sources: u64,
}
pub(super) fn identity(file: &File) -> Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata()?;
        if !meta.is_file() {
            bail!("collector file source must be a regular file");
        }
        let born = meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |t| t.as_nanos());
        Ok(format!("{}:{}:{born}", meta.dev(), meta.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        bail!("rotation-aware file capture requires Unix file identity")
    }
}
