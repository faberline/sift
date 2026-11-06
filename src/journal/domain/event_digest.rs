//! The digests that identify an event and a dedupe record, and their shard.

use sha2::{Digest, Sha256};

pub(in crate::journal) use crate::shared_kernel::event_content_digest::xor_digest_in_place;

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

#[cfg(test)]
mod tests {
    use super::{dedupe_record_digest, event_digest, shard_for};

    #[test]
    fn dedupe_event_digest_keeps_project_and_format_prefix() {
        let digest = event_digest("project-a", "log-fixed-001");
        assert_eq!(
            hex::encode(digest),
            "836b9aa112dcc85fcb7342cd8fe5a70daf25145cf85fcbaf9e290f3699fdecef"
        );
        assert_eq!(shard_for(&digest), 2102);
        assert_ne!(digest, event_digest("project-b", "log-fixed-001"));
        assert_ne!(event_digest("a", "bc"), event_digest("ab", "c"));
    }

    #[test]
    fn dedupe_record_digest_keeps_prefix_and_little_endian_fields() {
        let digest =
            hex::decode("836b9aa112dcc85fcb7342cd8fe5a70daf25145cf85fcbaf9e290f3699fdecef")
                .unwrap()
                .try_into()
                .unwrap();
        assert_eq!(
            hex::encode(dedupe_record_digest(-17, 4095, &(digest, 123456789, -42))),
            "4d538a81bbcecdd1ae9a5a77a70fadca8f360ec2c8d795b4cf2656ef0e1ee1b3"
        );
    }
}
