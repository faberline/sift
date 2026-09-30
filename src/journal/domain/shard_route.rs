//! Epoch shard routing: the 4096 virtual buckets, the epoch maps that assign
//! them to shards from an activation cursor on, and the route an event id
//! takes.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

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

#[derive(Debug, Error)]
pub(in crate::journal) enum EpochMapError {
    #[error("epoch map history must not be empty")]
    Empty,
    #[error("epoch ids must be strictly increasing")]
    EpochOrder,
    #[error("epoch {epoch} has {buckets} buckets; expected {expected}", expected = VIRTUAL_BUCKETS)]
    BucketCount { epoch: u64, buckets: usize },
    #[error("epoch activation cursors must be strictly increasing")]
    CursorOrder,
}

pub(in crate::journal) fn validate_epochs(epochs: &[EpochMap]) -> Result<(), EpochMapError> {
    if epochs.is_empty() {
        return Err(EpochMapError::Empty);
    }
    let mut previous_epoch = 0;
    let mut previous_cursor = None;
    for epoch in epochs {
        if epoch.epoch <= previous_epoch {
            return Err(EpochMapError::EpochOrder);
        }
        if epoch.bucket_to_shard.len() != VIRTUAL_BUCKETS {
            return Err(EpochMapError::BucketCount {
                epoch: epoch.epoch,
                buckets: epoch.bucket_to_shard.len(),
            });
        }
        if let Some(previous) = previous_cursor {
            if epoch.activated_at_cursor <= previous {
                return Err(EpochMapError::CursorOrder);
            }
        }
        previous_epoch = epoch.epoch;
        previous_cursor = Some(epoch.activated_at_cursor);
    }
    Ok(())
}
