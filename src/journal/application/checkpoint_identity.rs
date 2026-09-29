//! The identity a checkpoint of the journal is taken at.

use anyhow::{bail, Context, Result};

use crate::journal::domain::event_content_digest::xor_event_content_digest;
use crate::journal::domain::event_query::EventQuery;
use crate::journal::domain::journal_limits::RECOVERY_PAGE_EVENTS;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::shared_kernel::event_content_digest::xor_digest;

impl DurableJournal {
    pub(crate) fn checkpoint_identity(&self, raw_cursor: u64) -> Result<(u64, [u8; 32], u64)> {
        let (last_cursor, total_events, total_digest, retention_generation) = {
            let state = self.state.read().expect("journal state lock poisoned");
            (
                state.last_cursor,
                state.total_events,
                state.event_content_digest,
                state.retention_generation,
            )
        };
        if raw_cursor > last_cursor {
            bail!("checkpoint cursor is ahead of the Sift journal");
        }
        let mut suffix_count = 0_u64;
        let mut suffix_digest = [0_u8; 32];
        let mut after = raw_cursor;
        loop {
            let page = self.query_unchecked(EventQuery {
                signal: None,
                after,
                limit: RECOVERY_PAGE_EVENTS,
            })?;
            let Some(last) = page.last().map(|event| event.cursor) else {
                break;
            };
            for event in page {
                suffix_count = suffix_count.saturating_add(1);
                xor_event_content_digest(&mut suffix_digest, &event.event)?;
            }
            if last <= after {
                bail!("checkpoint suffix scan made no progress");
            }
            after = last;
        }
        if suffix_count != last_cursor.saturating_sub(raw_cursor) {
            bail!("checkpoint suffix is not a contiguous Raft cursor range");
        }
        let prefix_events = total_events
            .checked_sub(suffix_count)
            .context("checkpoint suffix exceeds retained event count")?;
        Ok((
            prefix_events,
            xor_digest(total_digest, suffix_digest),
            retention_generation,
        ))
    }
}
