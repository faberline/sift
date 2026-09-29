//! The digests that identify an event and a dedupe record, and their shard.

use sha2::{Digest, Sha256};

const SHARD_COUNT: usize = 4096;

pub(in crate::journal) fn dedupe_record_digest(
    generation: i64,
    shard: usize,
    (digest, cursor, acknowledged_at): &([u8; 32], u64, i64),
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"sift-dedupe-record-v1\0");
    hasher.update(generation.to_le_bytes());
    hasher.update((shard as u64).to_le_bytes());
    hasher.update(digest);
    hasher.update(cursor.to_le_bytes());
    hasher.update(acknowledged_at.to_le_bytes());
    hasher.finalize().into()
}

pub(in crate::journal) fn xor_digest_in_place(target: &mut [u8; 32], value: [u8; 32]) {
    for (target, value) in target.iter_mut().zip(value) {
        *target ^= value;
    }
}

pub(in crate::journal) fn event_digest(project: &str, event_id: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"sift-dedupe-v6\0");
    hasher.update((project.len() as u64).to_be_bytes());
    hasher.update(project.as_bytes());
    hasher.update((event_id.len() as u64).to_be_bytes());
    hasher.update(event_id.as_bytes());
    hasher.finalize().into()
}

pub(in crate::journal) fn shard_for(digest: &[u8; 32]) -> usize {
    (((digest[0] as usize) << 4) | ((digest[1] as usize) >> 4)) % SHARD_COUNT
}
