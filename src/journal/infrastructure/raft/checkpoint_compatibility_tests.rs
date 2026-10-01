//! The three checkpoint formats captured on 5ae45b9.

use super::archive_checkpoint::{
    read_archive_checkpoint, write_archive_checkpoint, ArchiveCheckpointV1,
};
use super::local_checkpoint::{read_local_checkpoint, write_local_checkpoint, LocalCheckpointV1};
use super::resident_checkpoint::{
    read_resident_checkpoint, write_resident_checkpoint, ResidentCheckpointV1,
};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/refactor_compatibility"
);

fn check<T: serde::Serialize>(
    name: &str,
    value: T,
    write: impl Fn(&T, &mut dyn std::io::Write) -> anyhow::Result<()>,
    read: impl Fn(&mut dyn std::io::Read) -> anyhow::Result<T>,
    label: &str,
) {
    let mut bytes = Vec::new();
    write(&value, &mut bytes).unwrap();
    let path = std::path::Path::new(FIXTURES).join(name);
    let old = std::fs::read(path).unwrap();
    assert_eq!(bytes, old);
    let decoded = read(&mut old.as_slice()).unwrap();
    let mut encoded = Vec::new();
    write(&decoded, &mut encoded).unwrap();
    assert_eq!(encoded, old);

    for (invalid, expected) in [
        (
            old.clone(),
            format!("invalid Sift {label} checkpoint magic"),
        ),
        (
            old.clone(),
            format!("Sift {label} checkpoint payload has an invalid length"),
        ),
        (
            old.clone(),
            format!("Sift {label} checkpoint checksum mismatch"),
        ),
        (old.clone(), format!("read Sift {label} checkpoint payload")),
        (
            old.clone(),
            format!("Sift {label} checkpoint contains trailing bytes"),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, pair)| {
        let (mut data, message) = pair;
        match i {
            0 => data[0] ^= 1,
            1 => data[8..12].copy_from_slice(&0u32.to_le_bytes()),
            2 => data[12] ^= 1,
            3 => {
                data.pop();
            }
            4 => data.push(0),
            _ => unreachable!(),
        }
        (data, message)
    }) {
        let err = read(&mut invalid.as_slice()).err().unwrap();
        assert_eq!(err.to_string(), expected);
    }
    let header_error = read(&mut &old[..15]).err().unwrap();
    assert_eq!(
        header_error.to_string(),
        format!("read Sift {label} checkpoint header")
    );
    let mut oversized = old.clone();
    oversized[8..12].copy_from_slice(&65537u32.to_le_bytes());
    assert_eq!(
        read(&mut oversized.as_slice()).err().unwrap().to_string(),
        format!("Sift {label} checkpoint payload has an invalid length")
    );
    for (limit, field) in [
        (0, "magic"),
        (8, "length"),
        (12, "checksum"),
        (16, "payload"),
    ] {
        let error = write(&value, &mut FailAfter(limit)).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("write Sift {label} checkpoint {field}")
        );
        assert_eq!(
            error.chain().last().unwrap().to_string(),
            "fixed write failure"
        );
    }
}

struct FailAfter(usize);

impl std::io::Write for FailAfter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0 == 0 {
            return Err(std::io::Error::other("fixed write failure"));
        }
        let count = self.0.min(bytes.len());
        self.0 -= count;
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn fence() -> serde_json::Value {
    serde_json::json!({"source_manifest_uri":"gs://bucket/old/manifest.json",
        "source_manifest_sha256":"d".repeat(64),"target_generation":7,
        "evaluate_at":"2026-09-30T12:34:56.123456789Z"})
}

#[test]
fn archive_checkpoint_keeps_bytes_and_rejections() {
    let checkpoint: ArchiveCheckpointV1 = serde_json::from_value(serde_json::json!({
        "format_version":2,"applied_index":19,"raw_cursor":3,"archive_snapshot_index":3,
        "watermarks":{"logs":1,"metrics":2,"traces":3},
        "manifest_uri":"gs://bucket/fixed/manifest.json","manifest_sha256":"a".repeat(64),
        "retention_generation":7,"pending_retention":fence(),"archive_gc_authorized":false
    }))
    .unwrap();
    check(
        "checkpoint-archive.bin",
        checkpoint,
        write_archive_checkpoint,
        read_archive_checkpoint,
        "archive",
    );
}

#[test]
fn local_checkpoint_keeps_bytes_and_rejections() {
    let checkpoint: LocalCheckpointV1 = serde_json::from_value(serde_json::json!({
        "format_version":2,"applied_index":19,"raw_cursor":3,"local_snapshot_index":3,
        "watermarks":{"logs":1,"metrics":2,"traces":3},"pending_retention":fence()
    }))
    .unwrap();
    check(
        "checkpoint-local.bin",
        checkpoint,
        write_local_checkpoint,
        read_local_checkpoint,
        "local",
    );
}

#[test]
fn resident_checkpoint_keeps_bytes_and_rejections() {
    let checkpoint: ResidentCheckpointV1 = serde_json::from_value(serde_json::json!({
        "format_version":2,"applied_index":19,"raw_cursor":3,"event_count":3,
        "retention_generation":7,"event_content_sha256":"b".repeat(64),"pending_retention":fence()
    }))
    .unwrap();
    check(
        "checkpoint-resident.bin",
        checkpoint,
        write_resident_checkpoint,
        read_resident_checkpoint,
        "resident",
    );
}
