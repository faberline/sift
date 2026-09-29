//! Whether a query read the archive, and the stats and warnings that follow.

use crate::query::interfaces::http::query_response_v1::QueryResponseV1;

#[derive(Debug)]
pub(crate) enum ArchiveQueryStatus {
    NotRequired,
    Ready(crate::archive::application::archive_receipts::ArchiveReplay),
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
