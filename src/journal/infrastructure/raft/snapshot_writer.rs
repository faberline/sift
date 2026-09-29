//! Writing the journal out as a snapshot stream.

use std::io::Write;

use anyhow::{bail, Context, Result};

use crate::journal::domain::event_query::EventQuery;
use crate::journal::infrastructure::durable_journal::DurableJournal;
use crate::journal::infrastructure::raft::snapshot_format::{
    write_snapshot_event, write_snapshot_header, SnapshotMetadata, SNAPSHOT_PAGE_EVENTS,
};

/// Write one stable journal prefix as framed binary data.
///
/// The writer receives one event at a time. A concurrent append can extend the
/// journal, but it cannot change the prefix captured in the header.
pub(crate) fn write_snapshot(
    journal: &DurableJournal,
    applied_index: u64,
    writer: &mut dyn Write,
) -> Result<SnapshotMetadata> {
    let (last_cursor, event_count) = journal.snapshot_bounds();
    let metadata = SnapshotMetadata {
        applied_index,
        last_cursor,
        event_count,
        pending_retention: None,
    };
    write_snapshot_header(writer, &metadata)?;

    let mut after = 0_u64;
    let mut written = 0_u64;
    let mut expected_cursor = 1_u64;
    while written < event_count {
        let page = journal
            .query_unchecked(EventQuery {
                signal: None,
                after,
                limit: SNAPSHOT_PAGE_EVENTS,
            })
            .context("read canonical journal page for snapshot")?;
        let mut progressed = false;
        for stored in page {
            if stored.cursor > last_cursor || written == event_count {
                break;
            }
            if stored.cursor != expected_cursor {
                bail!(
                    "snapshot journal cursor {} is out of order; expected {expected_cursor}",
                    stored.cursor
                );
            }
            write_snapshot_event(writer, &stored)?;
            after = stored.cursor;
            expected_cursor = expected_cursor
                .checked_add(1)
                .context("snapshot journal cursor exhausted u64")?;
            written = written
                .checked_add(1)
                .context("snapshot event count exhausted u64")?;
            progressed = true;
        }
        if !progressed {
            bail!(
                "snapshot ended after {written} events, before declared event count {event_count}"
            );
        }
    }
    if written != event_count || after != last_cursor {
        bail!(
            "snapshot prefix mismatch: wrote {written} events through cursor {after}, expected {event_count} events through cursor {last_cursor}"
        );
    }
    Ok(metadata)
}
