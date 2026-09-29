//! The snapshot stream format: its content type, header and metadata, and the
//! framed records inside it.

use std::io::{Read, Write};

use anyhow::{bail, Context, Result};

use crate::journal::domain::retention_fence::RetentionFenceV1;
use crate::shared_kernel::stored_event::StoredEvent;

pub const SNAPSHOT_CONTENT_TYPE: &str = "application/vnd.axiom.sift-snapshot";

const SNAPSHOT_MAGIC: &[u8; 8] = b"SIFTSNP2";

const SNAPSHOT_FORMAT_VERSION: u16 = 2;

const SNAPSHOT_HEADER_BYTES: usize = 40;

pub(super) const SNAPSHOT_FRAME_HEADER_BYTES: usize = 16;

pub(super) const SNAPSHOT_PAGE_EVENTS: usize = 10_000;

pub(super) const MAX_SNAPSHOT_EVENT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SnapshotMetadata {
    pub applied_index: u64,
    pub last_cursor: u64,
    pub event_count: u64,
    /// `None` means the legacy full snapshot had no control-state field.
    /// `Some(None)` is an authoritative cleared fence.
    pub pending_retention: Option<Option<RetentionFenceV1>>,
}

pub(super) fn write_snapshot_header(
    writer: &mut dyn Write,
    metadata: &SnapshotMetadata,
) -> Result<()> {
    let mut header = Vec::with_capacity(SNAPSHOT_HEADER_BYTES);
    header.extend_from_slice(SNAPSHOT_MAGIC);
    header.extend_from_slice(&SNAPSHOT_FORMAT_VERSION.to_le_bytes());
    header.extend_from_slice(&0_u16.to_le_bytes());
    header.extend_from_slice(&metadata.applied_index.to_le_bytes());
    header.extend_from_slice(&metadata.last_cursor.to_le_bytes());
    header.extend_from_slice(&metadata.event_count.to_le_bytes());
    let checksum = crc32fast::hash(&header);
    header.extend_from_slice(&checksum.to_le_bytes());
    debug_assert_eq!(header.len(), SNAPSHOT_HEADER_BYTES);
    writer
        .write_all(&header)
        .context("write Sift snapshot header")
}

pub(super) fn write_snapshot_event(writer: &mut dyn Write, stored: &StoredEvent) -> Result<()> {
    let payload = serde_json::to_vec(stored).context("encode Sift snapshot event")?;
    if payload.len() > MAX_SNAPSHOT_EVENT_BYTES {
        bail!(
            "Sift snapshot event at cursor {} exceeds the {} byte limit",
            stored.cursor,
            MAX_SNAPSHOT_EVENT_BYTES
        );
    }
    let payload_len = u32::try_from(payload.len()).context("snapshot event length exceeds u32")?;
    let checksum = crc32fast::hash(&payload);
    writer
        .write_all(&stored.cursor.to_le_bytes())
        .context("write Sift snapshot event cursor")?;
    writer
        .write_all(&payload_len.to_le_bytes())
        .context("write Sift snapshot event length")?;
    writer
        .write_all(&checksum.to_le_bytes())
        .context("write Sift snapshot event checksum")?;
    writer
        .write_all(&payload)
        .context("write Sift snapshot event payload")
}

pub(super) fn read_snapshot_header(reader: &mut dyn Read) -> Result<SnapshotMetadata> {
    let mut header = [0_u8; SNAPSHOT_HEADER_BYTES];
    reader
        .read_exact(&mut header)
        .context("read Sift snapshot header")?;
    if &header[..SNAPSHOT_MAGIC.len()] != SNAPSHOT_MAGIC {
        if matches!(header.first(), Some(b'{') | Some(b'[')) {
            bail!("legacy Sift JSON snapshot is unsupported; create a new empty data directory");
        }
        bail!("invalid Sift snapshot magic");
    }
    let version = u16::from_le_bytes(header[8..10].try_into().unwrap());
    if version != SNAPSHOT_FORMAT_VERSION {
        bail!("unsupported Sift snapshot format version {version}");
    }
    let flags = u16::from_le_bytes(header[10..12].try_into().unwrap());
    if flags != 0 {
        bail!("unsupported Sift snapshot flags {flags}");
    }
    let expected_checksum = u32::from_le_bytes(header[36..40].try_into().unwrap());
    let actual_checksum = crc32fast::hash(&header[..36]);
    if actual_checksum != expected_checksum {
        bail!("Sift snapshot header checksum mismatch");
    }
    Ok(SnapshotMetadata {
        applied_index: u64::from_le_bytes(header[12..20].try_into().unwrap()),
        last_cursor: u64::from_le_bytes(header[20..28].try_into().unwrap()),
        event_count: u64::from_le_bytes(header[28..36].try_into().unwrap()),
        pending_retention: None,
    })
}

pub(super) fn read_one_or_eof(reader: &mut dyn Read) -> Result<Option<u8>> {
    let mut byte = [0_u8; 1];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => return Ok(None),
            Ok(_) => return Ok(Some(byte[0])),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error).context("check Sift snapshot end"),
        }
    }
}
