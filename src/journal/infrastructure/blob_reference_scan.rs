//! Scanning the journal for blob references before blob garbage collection.

use std::collections::BTreeSet;

use anyhow::{bail, Result};

use crate::journal::domain::journal_limits::RECOVERY_PAGE_BYTES;
use crate::journal::infrastructure::canonical_recovery_reader::CanonicalRecoveryReader;
use crate::journal::infrastructure::durable_journal::DurableJournal;

impl DurableJournal {
    pub(crate) fn scan_blob_references_page(
        &self,
        after: u64,
        limit: usize,
    ) -> Result<(Vec<String>, u64, bool)> {
        if limit == 0 {
            bail!("blob reference scan limit must be greater than zero");
        }
        let archived = crate::archive::application::archive_status_queries::committed_watermarks(
            self.data_dir(),
        )?;
        let mut reader =
            CanonicalRecoveryReader::open(&self.storage, &self.wal, archived, after, true)?;
        let (page, exhausted) = reader.read_page_with_limits(limit, RECOVERY_PAGE_BYTES)?;
        let scanned_through = page.last().map(|event| event.cursor).unwrap_or_else(|| {
            if exhausted {
                self.last_cursor()
            } else {
                after
            }
        });
        let mut references = BTreeSet::new();
        for event in page {
            references.extend(
                event
                    .event
                    .blob_refs
                    .into_iter()
                    .map(|reference| reference.hash),
            );
        }
        Ok((references.into_iter().collect(), scanned_through, exhausted))
    }

    pub(crate) fn finalize_blob_candidates_with_index<Mark, IsLive>(
        &self,
        hashes: &[String],
        after: u64,
        limit: usize,
        mut mark_live: Mark,
        mut is_live: IsLive,
    ) -> Result<(u64, usize, bool)>
    where
        Mark: FnMut(&str) -> Result<()>,
        IsLive: FnMut(&str) -> Result<bool>,
    {
        let _blob_gate = self.blob_gate.lock().expect("Sift blob gate poisoned");
        let (references, scanned_through, exhausted) =
            self.scan_blob_references_page(after, limit)?;
        for hash in references {
            mark_live(&hash)?;
        }
        if !exhausted {
            return Ok((scanned_through, 0, false));
        }
        let mut removed = 0_usize;
        for hash in hashes {
            if !is_live(hash)? && self.storage.remove_blob(hash)? {
                removed = removed.saturating_add(1);
            }
        }
        Ok((self.last_cursor(), removed, true))
    }
}
