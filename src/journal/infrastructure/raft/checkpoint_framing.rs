//! The shared 16-byte framing for archive, local and resident checkpoints.

use std::io::{Read, Write};

use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Serialize};

use super::snapshot_format::read_one_or_eof;

const HEADER_BYTES: usize = 16;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

pub(super) struct CheckpointFrame {
    pub(super) magic: &'static [u8; 8],
    pub(super) errors: FrameErrors,
}

pub(super) struct FrameErrors {
    pub(super) encode: &'static str,
    pub(super) invalid_length: &'static str,
    pub(super) length_overflow: &'static str,
    pub(super) write_magic: &'static str,
    pub(super) write_length: &'static str,
    pub(super) write_checksum: &'static str,
    pub(super) write_payload: &'static str,
    pub(super) read_header: &'static str,
    pub(super) invalid_magic: &'static str,
    pub(super) read_payload: &'static str,
    pub(super) checksum_mismatch: &'static str,
    pub(super) trailing_bytes: &'static str,
    pub(super) decode: &'static str,
}

impl CheckpointFrame {
    pub(super) fn write<T: Serialize>(&self, value: &T, writer: &mut dyn Write) -> Result<()> {
        let payload = serde_json::to_vec(value).context(self.errors.encode)?;
        if payload.is_empty() || payload.len() > MAX_PAYLOAD_BYTES {
            bail!(self.errors.invalid_length);
        }
        let payload_len = u32::try_from(payload.len()).context(self.errors.length_overflow)?;
        writer
            .write_all(self.magic)
            .context(self.errors.write_magic)?;
        writer
            .write_all(&payload_len.to_le_bytes())
            .context(self.errors.write_length)?;
        writer
            .write_all(&crc32fast::hash(&payload).to_le_bytes())
            .context(self.errors.write_checksum)?;
        writer
            .write_all(&payload)
            .context(self.errors.write_payload)
    }

    pub(super) fn read<T: DeserializeOwned>(&self, reader: &mut dyn Read) -> Result<T> {
        let mut header = [0_u8; HEADER_BYTES];
        reader
            .read_exact(&mut header)
            .context(self.errors.read_header)?;
        if &header[..8] != self.magic {
            bail!(self.errors.invalid_magic);
        }
        let payload_len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
        let expected_checksum = u32::from_le_bytes(header[12..16].try_into().unwrap());
        if payload_len == 0 || payload_len > MAX_PAYLOAD_BYTES {
            bail!(self.errors.invalid_length);
        }
        let mut payload = vec![0_u8; payload_len];
        reader
            .read_exact(&mut payload)
            .context(self.errors.read_payload)?;
        if crc32fast::hash(&payload) != expected_checksum {
            bail!(self.errors.checksum_mismatch);
        }
        if read_one_or_eof(reader)?.is_some() {
            bail!(self.errors.trailing_bytes);
        }
        serde_json::from_slice(&payload).context(self.errors.decode)
    }
}
