//! The stable id an OTLP item without its own sift.event_id is given.

use sha2::{Digest, Sha256};

pub(in crate::ingest) fn stable_id(kind: &str, project: &str, identity: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(kind.as_bytes());
    hash.update([0]);
    hash.update(project.as_bytes());
    hash.update([0]);
    hash.update(identity.as_bytes());
    format!("otlp-{kind}-{}", hex::encode(&hash.finalize()[..16]))
}
