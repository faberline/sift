//! The lowercase hex SHA-256 digest Sift uses for content hashes, object keys
//! and snapshot fingerprints.

use sha2::{Digest, Sha256};

pub(crate) fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(bytes.as_ref()))
}
