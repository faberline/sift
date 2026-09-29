//! Archiving raw storage to GCS after checking the local archive prefix.

use anyhow::{bail, Context, Result};

use crate::archive::application::archive_receipts::ArchiveReceipt;
use crate::archive::application::upload_archive_snapshot::archive_gcs_captured_streaming;
use crate::journal::infrastructure::storage::raw_storage::RawStorage;
use crate::storage::SegmentManifest;
use crate::SignalKind;

/// Upload every immutable object first. The manifest upload is the commit point.
pub fn archive_gcs(storage: &RawStorage, destination_uri: &str) -> Result<ArchiveReceipt> {
    let segments = storage.seal_all_with_signal()?;
    let captured_cursor = segments
        .iter()
        .map(|(_, manifest)| manifest.last_cursor)
        .max()
        .unwrap_or_default();
    archive_gcs_captured(storage, destination_uri, captured_cursor, segments)
}

pub(super) fn archive_gcs_captured(
    storage: &RawStorage,
    destination_uri: &str,
    captured_cursor: u64,
    captured_segments: Vec<(SignalKind, SegmentManifest)>,
) -> Result<ArchiveReceipt> {
    archive_gcs_captured_streaming(storage, destination_uri, captured_cursor, captured_segments)
}

pub(super) fn verify_local_archive_prefix(
    storage: &RawStorage,
    previously_covered: u64,
    captured_cursor: u64,
) -> Result<()> {
    let mut after = previously_covered;
    while after < captured_cursor {
        let page = storage.query_events(None, after, 10_000)?;
        let mut progressed = false;
        for event in page {
            if event.cursor > captured_cursor {
                break;
            }
            let expected = after
                .checked_add(1)
                .context("archive cursor exhausted u64")?;
            if event.cursor != expected {
                bail!(
                    "archive prefix is not contiguous: expected cursor {expected}, found {}",
                    event.cursor
                );
            }
            after = event.cursor;
            progressed = true;
        }
        if !progressed {
            bail!(
                "archive prefix ended at cursor {after}, before captured cursor {captured_cursor}"
            );
        }
    }
    Ok(())
}
