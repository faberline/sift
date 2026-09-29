//! Epoch shard routing: the 4096 virtual buckets, the epoch maps that assign
//! them to shards from an activation cursor on, and the route an event id
//! takes.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const VIRTUAL_BUCKETS: usize = 4_096;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EpochMap {
    pub epoch: u64,
    pub activated_at_cursor: u64,
    pub bucket_to_shard: Vec<u16>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Route {
    pub epoch: u64,
    pub shard: u16,
    pub bucket: u16,
}

pub(crate) fn bucket_for(event_id: &str) -> u16 {
    let digest = Sha256::digest(event_id.as_bytes());
    u16::from_be_bytes([digest[0], digest[1]]) & 0x0fff
}

pub(in crate::journal) fn validate_epochs(epochs: &[EpochMap]) -> Result<()> {
    if epochs.is_empty() {
        bail!("epoch map history must not be empty");
    }
    let mut previous_epoch = 0;
    let mut previous_cursor = None;
    for epoch in epochs {
        if epoch.epoch <= previous_epoch {
            bail!("epoch ids must be strictly increasing");
        }
        if epoch.bucket_to_shard.len() != VIRTUAL_BUCKETS {
            bail!(
                "epoch {} has {} buckets; expected {VIRTUAL_BUCKETS}",
                epoch.epoch,
                epoch.bucket_to_shard.len()
            );
        }
        if let Some(previous) = previous_cursor {
            if epoch.activated_at_cursor <= previous {
                bail!("epoch activation cursors must be strictly increasing");
            }
        }
        previous_epoch = epoch.epoch;
        previous_cursor = Some(epoch.activated_at_cursor);
    }
    Ok(())
}
