//! A query's times: whether it reaches the archive, and parsing a query time.

use chrono::{DateTime, Utc};

use crate::query::domain::query_failure::QueryFailure;
use crate::shared_kernel::query_request::QueryRequestV1;

pub(in crate::query) fn cold_query_requested_at(
    request: &QueryRequestV1,
    now: DateTime<Utc>,
) -> bool {
    let Some(start) = request.time_range.start.as_deref() else {
        return false;
    };
    DateTime::parse_from_rfc3339(start)
        .map(|start| start.with_timezone(&Utc) < now - chrono::Duration::days(30))
        .unwrap_or(false)
}

pub(in crate::query) fn parse_query_time_nanos(value: &str) -> Result<u64, QueryFailure> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| QueryFailure::bad_request("invalid_query", "time range must be RFC 3339"))?;
    let nanos = parsed.timestamp_nanos_opt().ok_or_else(|| {
        QueryFailure::bad_request("invalid_query", "time range is outside the supported range")
    })?;
    u64::try_from(nanos).map_err(|_| {
        QueryFailure::bad_request("invalid_query", "time range must not precede Unix epoch")
    })
}

#[cfg(test)]
mod tests {
    use super::parse_query_time_nanos;

    #[test]
    fn invalid_times_keep_the_public_code_and_message() {
        for (value, message) in [
            ("invalid", "time range must be RFC 3339"),
            (
                "9999-01-01T00:00:00Z",
                "time range is outside the supported range",
            ),
            (
                "1969-12-31T23:59:59Z",
                "time range must not precede Unix epoch",
            ),
        ] {
            let error = parse_query_time_nanos(value).unwrap_err();
            assert_eq!(error.code, "invalid_query");
            assert_eq!(error.message, message);
            assert_eq!(error.to_string(), message);
        }
    }
}
