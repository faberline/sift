//! Whether a query read the archive, and the stats and warnings that follow.

use crate::shared_kernel::query_response::QueryResponseV1;

#[derive(Debug)]
pub(crate) enum ArchiveQueryStatus {
    NotRequired,
    Ready(crate::shared_kernel::archive_receipts::ArchiveReplay),
    Unavailable(String),
}

pub(super) fn apply_archive_query_status(
    mut response: QueryResponseV1,
    status: ArchiveQueryStatus,
) -> QueryResponseV1 {
    match status {
        ArchiveQueryStatus::NotRequired => {}
        ArchiveQueryStatus::Ready(replay) => {
            if replay.replayed == 0 && replay.scanned > 0 {
                response
                    .warnings
                    .push("archive scan completed with no matching events".to_string());
            }
        }
        ArchiveQueryStatus::Unavailable(warning) => {
            response.partial = true;
            response.warnings.push(warning);
        }
    }
    response
}
