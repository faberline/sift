//! The CRI source's checkpoint (collector.cri.checkpoint.v1): per-file offsets
//! keyed by device and inode, bound to one root, and the loss counters.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::collector::domain::cri::WorkloadIdentity;

const CRI_CHECKPOINT_SCHEMA: &str = "collector.cri.checkpoint.v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct CriCheckpoint {
    schema: String,
    pub(super) root: String,
    pub(super) files: BTreeMap<String, CriFileCheckpoint>,
    pub(super) accepted: u64,
    pub(super) duplicates: u64,
    pub(super) rejected: u64,
    pub(super) lost_bytes: u64,
    pub(super) lost_sources: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct CriFileCheckpoint {
    pub(super) offset: u64,
    pub(super) line: u64,
    pub(super) observed_len: u64,
    pub(super) relative_path: String,
    pub(super) workload: WorkloadIdentity,
    pub(super) retired: bool,
    pub(super) loss_reported: bool,
}

impl CriCheckpoint {
    fn new(root: &str) -> Self {
        Self {
            schema: CRI_CHECKPOINT_SCHEMA.to_string(),
            root: root.to_string(),
            files: BTreeMap::new(),
            accepted: 0,
            duplicates: 0,
            rejected: 0,
            lost_bytes: 0,
            lost_sources: 0,
        }
    }

    pub(super) fn load(path: &Path, root: &str) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new(root));
        }
        let checkpoint: Self = service_collector::load_json_checkpoint(path)?
            .context("CRI checkpoint disappeared while loading")?;
        if checkpoint.schema != CRI_CHECKPOINT_SCHEMA {
            bail!(
                "unsupported CRI checkpoint schema {}; expected {CRI_CHECKPOINT_SCHEMA}",
                checkpoint.schema
            );
        }
        if checkpoint.root != root {
            bail!(
                "CRI checkpoint root mismatch: stored {}, configured {root}",
                checkpoint.root
            );
        }
        Ok(checkpoint)
    }

    pub(super) fn save(&self, path: &Path) -> Result<()> {
        service_collector::save_json_checkpoint(path, self)
    }
}
