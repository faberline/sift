//! Encoding, compressing and decoding the replicated Sift command.

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Read, Write};

use crate::journal::domain::raft_batch_limits::RAFT_BATCH_MAX_BYTES;
use crate::journal::domain::sift_command::SiftCommandV1;

pub(super) const COMMAND_MAGIC: &[u8; 8] = b"SIFTCMD1";

const COMMAND_FORMAT_VERSION: u16 = 2;

const COMMAND_FLAG_GZIP: u16 = 1;

const COMMAND_HEADER_BYTES: usize = 20;

pub(super) const MAX_ENCODED_COMMAND_BYTES: usize = RAFT_BATCH_MAX_BYTES + 64 * 1024;

impl SiftCommandV1 {
    pub(crate) fn encoded(&self) -> Result<Vec<u8>> {
        // MVP deployments run one immutable candidate digest on every voter.
        // The v6 append record intentionally adds the shared decision time;
        // Sift 0.1.1 command compatibility is not retained.
        self.uncompressed()
    }

    pub(super) fn encoded_compressed(&self) -> Result<Vec<u8>> {
        let raw = self.uncompressed()?;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder
            .write_all(&raw)
            .context("compress Sift state-machine command")?;
        let compressed = encoder
            .finish()
            .context("finish Sift state-machine command compression")?;
        let mut encoded = Vec::with_capacity(COMMAND_HEADER_BYTES + compressed.len());
        encoded.extend_from_slice(COMMAND_MAGIC);
        encoded.extend_from_slice(&COMMAND_FORMAT_VERSION.to_le_bytes());
        encoded.extend_from_slice(&COMMAND_FLAG_GZIP.to_le_bytes());
        encoded.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        encoded.extend_from_slice(&crc32fast::hash(&raw).to_le_bytes());
        encoded.extend_from_slice(&compressed);
        if encoded.len() > MAX_ENCODED_COMMAND_BYTES {
            bail!(
                "encoded Sift Raft batch exceeds the wire limit: {} bytes",
                encoded.len()
            );
        }
        Ok(encoded)
    }

    pub(crate) fn uncompressed_len(&self) -> Result<usize> {
        Ok(self.uncompressed()?.len())
    }

    fn uncompressed(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self).context("encode Sift state-machine command")?;
        if bytes.len() > RAFT_BATCH_MAX_BYTES {
            bail!(
                "Sift Raft batch exceeds the 1 MiB limit: {} bytes",
                bytes.len()
            );
        }
        Ok(bytes)
    }
}

pub(super) fn decode_command(bytes: &[u8]) -> Result<SiftCommandV1> {
    if !bytes.starts_with(COMMAND_MAGIC) {
        if bytes.len() > RAFT_BATCH_MAX_BYTES {
            bail!("legacy Sift Raft batch exceeds the 1 MiB limit");
        }
        return serde_json::from_slice(bytes).context("decode legacy Sift command v1");
    }
    if bytes.len() < COMMAND_HEADER_BYTES {
        bail!("compressed Sift Raft batch header is truncated");
    }
    let version = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
    let flags = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
    let expected_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let expected_crc = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    if version != COMMAND_FORMAT_VERSION {
        bail!("unsupported Sift Raft command format {version}");
    }
    if flags != COMMAND_FLAG_GZIP {
        bail!("unsupported Sift Raft command flags {flags}");
    }
    if expected_len > RAFT_BATCH_MAX_BYTES {
        bail!("compressed Sift Raft batch declares more than 1 MiB");
    }
    let mut decoder = GzDecoder::new(&bytes[COMMAND_HEADER_BYTES..]);
    let mut raw = Vec::with_capacity(expected_len.min(RAFT_BATCH_MAX_BYTES));
    decoder
        .by_ref()
        .take((RAFT_BATCH_MAX_BYTES + 1) as u64)
        .read_to_end(&mut raw)
        .context("decompress Sift Raft command")?;
    if raw.len() != expected_len {
        bail!(
            "compressed Sift Raft batch length mismatch: expected {expected_len}, found {}",
            raw.len()
        );
    }
    if crc32fast::hash(&raw) != expected_crc {
        bail!("compressed Sift Raft batch checksum mismatch");
    }
    serde_json::from_slice(&raw).context("decode compressed Sift command")
}
