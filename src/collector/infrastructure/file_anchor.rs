//! Check the bytes next to a saved position before reusing that position.
use crate::collector::domain::record::FileAnchor;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;

const ANCHOR_BYTES: usize = 256;

pub(super) fn keep_tail(tail: &mut Vec<u8>, bytes: &[u8]) {
    if bytes.len() >= ANCHOR_BYTES {
        tail.clear();
        tail.extend_from_slice(&bytes[bytes.len() - ANCHOR_BYTES..]);
    } else {
        tail.extend_from_slice(bytes);
        if tail.len() > ANCHOR_BYTES {
            tail.drain(..tail.len() - ANCHOR_BYTES);
        }
    }
}

pub(super) fn from_tail(tail: &[u8]) -> Option<FileAnchor> {
    if tail.is_empty() {
        return None;
    }
    let tail = &tail[tail.len().saturating_sub(ANCHOR_BYTES)..];
    Some(FileAnchor {
        width: tail.len() as u16,
        sha256: Sha256::digest(tail).into(),
    })
}

pub(super) fn matches(file: &File, offset: u64, anchor: Option<&FileAnchor>) -> Result<bool> {
    let Some(anchor) = anchor else {
        return Ok(true); // Older checkpoints have no evidence of these bytes.
    };
    let width = usize::from(anchor.width);
    if width == 0 || width > ANCHOR_BYTES || offset < width as u64 {
        bail!("invalid collector file anchor");
    }
    let mut bytes = [0_u8; ANCHOR_BYTES];
    #[cfg(unix)]
    let read = {
        use std::os::unix::fs::FileExt;
        file.read_exact_at(&mut bytes[..width], offset - width as u64)
    };
    #[cfg(not(unix))]
    let read = {
        use std::io::{Read, Seek, SeekFrom};
        let mut reader = file.try_clone()?;
        reader.seek(SeekFrom::Start(offset - width as u64))?;
        reader.read_exact(&mut bytes[..width])
    };
    match read {
        Ok(()) => Ok(<[u8; 32]>::from(Sha256::digest(&bytes[..width])) == anchor.sha256),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(error).context("read collector file anchor"),
    }
}

pub(super) fn current_generation(cursor: Option<u64>, current: u64) -> Result<bool> {
    match cursor {
        Some(value) if value > current => bail!("collector cursor has a future file generation"),
        Some(value) if value < current => Ok(false),
        _ => Ok(true),
    }
}
