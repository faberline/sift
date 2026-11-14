//! Property tests for the three checkpoint frames: round trips, arbitrary input
//! and bit flips.

use super::archive_checkpoint::{
    read_archive_checkpoint, write_archive_checkpoint, ArchiveCheckpointV1,
    ARCHIVE_CHECKPOINT_FORMAT_VERSION,
};
use super::local_checkpoint::{
    read_local_checkpoint, write_local_checkpoint, LocalCheckpointV1,
    LOCAL_CHECKPOINT_FORMAT_VERSION,
};
use super::resident_checkpoint::{
    read_resident_checkpoint, write_resident_checkpoint, ResidentCheckpointV1,
    RESIDENT_CHECKPOINT_FORMAT_VERSION,
};
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use proptest::prelude::*;

fn archive_checkpoint() -> impl Strategy<Value = ArchiveCheckpointV1> {
    (1u64..u64::MAX, 0u64..1_000_000)
        .prop_flat_map(|(applied_index, snapshot)| {
            (
                Just(applied_index),
                Just(snapshot),
                0..=snapshot,
                0..=snapshot,
                0..=snapshot,
                "[a-z0-9/]{1,20}",
                "[0-9a-f]{64}",
                any::<u64>(),
                any::<bool>(),
            )
        })
        .prop_map(
            |(applied_index, snapshot, logs, metrics, traces, path, sha, generation, gc)| {
                ArchiveCheckpointV1 {
                    format_version: ARCHIVE_CHECKPOINT_FORMAT_VERSION,
                    applied_index,
                    raw_cursor: snapshot,
                    archive_snapshot_index: snapshot,
                    watermarks: ArchiveWatermarks {
                        logs,
                        metrics,
                        traces,
                    },
                    manifest_uri: format!("gs://b/{path}"),
                    manifest_sha256: sha,
                    retention_generation: generation,
                    pending_retention: None,
                    archive_gc_authorized: gc,
                }
            },
        )
}

fn local_checkpoint() -> impl Strategy<Value = LocalCheckpointV1> {
    (1u64..u64::MAX, 0u64..1_000_000)
        .prop_flat_map(|(applied_index, snapshot)| {
            (
                Just(applied_index),
                Just(snapshot),
                0..=snapshot,
                0..=snapshot,
            )
        })
        .prop_map(
            |(applied_index, snapshot, metrics, traces)| LocalCheckpointV1 {
                format_version: LOCAL_CHECKPOINT_FORMAT_VERSION,
                applied_index,
                raw_cursor: snapshot,
                local_snapshot_index: snapshot,
                // The local validator needs the highest watermark to equal the snapshot.
                watermarks: ArchiveWatermarks {
                    logs: snapshot,
                    metrics,
                    traces,
                },
                pending_retention: None,
            },
        )
}

fn resident_checkpoint() -> impl Strategy<Value = ResidentCheckpointV1> {
    (1u64..u64::MAX, 1u64..1_000_000)
        .prop_flat_map(|(applied_index, raw_cursor)| {
            (
                Just(applied_index),
                Just(raw_cursor),
                1..=raw_cursor,
                any::<u64>(),
                "[0-9a-f]{64}",
            )
        })
        .prop_map(
            |(applied_index, raw_cursor, event_count, generation, sha)| ResidentCheckpointV1 {
                format_version: RESIDENT_CHECKPOINT_FORMAT_VERSION,
                applied_index,
                raw_cursor,
                event_count,
                retention_generation: generation,
                event_content_sha256: sha,
                pending_retention: None,
            },
        )
}

fn encode_archive(checkpoint: &ArchiveCheckpointV1) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_archive_checkpoint(checkpoint, &mut bytes).unwrap();
    bytes
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// write then read of a valid archive checkpoint returns an equal value and the same bytes.
    #[test]
    fn archive_checkpoint_round_trips(value in archive_checkpoint()) {
        let bytes = encode_archive(&value);
        let decoded = read_archive_checkpoint(&mut bytes.as_slice()).unwrap();
        prop_assert_eq!(&decoded, &value);
        prop_assert_eq!(encode_archive(&decoded), bytes);
    }

    /// write then read of a valid local checkpoint gives the same bytes when written again.
    #[test]
    fn local_checkpoint_round_trips(value in local_checkpoint()) {
        let mut bytes = Vec::new();
        write_local_checkpoint(&value, &mut bytes).unwrap();
        let decoded = read_local_checkpoint(&mut bytes.as_slice()).unwrap();
        let mut again = Vec::new();
        write_local_checkpoint(&decoded, &mut again).unwrap();
        prop_assert_eq!(again, bytes);
    }

    /// write then read of a valid resident checkpoint gives the same bytes when written again.
    #[test]
    fn resident_checkpoint_round_trips(value in resident_checkpoint()) {
        let mut bytes = Vec::new();
        write_resident_checkpoint(&value, &mut bytes).unwrap();
        let decoded = read_resident_checkpoint(&mut bytes.as_slice()).unwrap();
        let mut again = Vec::new();
        write_resident_checkpoint(&decoded, &mut again).unwrap();
        prop_assert_eq!(again, bytes);
    }

    /// Any byte string gives Ok or Err from the three checkpoint readers, never a panic.
    #[test]
    fn checkpoint_readers_never_panic_on_any_bytes(
        bytes in prop::collection::vec(any::<u8>(), 0..256),
        magic in prop_oneof![
            Just(*b"SIFTRCP1"),
            Just(*b"SIFTLCP1"),
            Just(*b"SIFTRSD1"),
        ],
    ) {
        let _ = read_archive_checkpoint(&mut bytes.as_slice());
        let _ = read_local_checkpoint(&mut bytes.as_slice());
        let _ = read_resident_checkpoint(&mut bytes.as_slice());

        let mut prefixed = magic.to_vec();
        prefixed.extend_from_slice(&bytes);
        let _ = read_archive_checkpoint(&mut prefixed.as_slice());
        let _ = read_local_checkpoint(&mut prefixed.as_slice());
        let _ = read_resident_checkpoint(&mut prefixed.as_slice());
    }

    /// Flipping any bit of a valid framed checkpoint makes the reader return Err.
    #[test]
    fn checkpoint_bit_flip_is_rejected(
        value in archive_checkpoint(),
        position in any::<prop::sample::Index>(),
        bit in 0u8..8,
    ) {
        let mut bytes = encode_archive(&value);
        let index = position.index(bytes.len());
        bytes[index] ^= 1 << bit;
        prop_assert!(read_archive_checkpoint(&mut bytes.as_slice()).is_err());
    }
}
