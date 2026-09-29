//! A query's times: whether it reaches the archive, and parsing a query time.

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::query::interfaces::http::query_request_v1::QueryRequestV1;
use crate::ApiError;

pub(in crate::query) fn cold_query_requested(request: &QueryRequestV1) -> bool {
    let Some(start) = request.time_range.start.as_deref() else {
        return false;
    };
    DateTime::parse_from_rfc3339(start)
        .map(|start| start.with_timezone(&Utc) < Utc::now() - chrono::Duration::days(30))
        .unwrap_or(false)
}

pub(in crate::query) fn parse_query_time_nanos(value: &str) -> Result<u64, ApiError> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| ApiError::bad_request("invalid_query", "time range must be RFC 3339"))?;
    let nanos = parsed.timestamp_nanos_opt().ok_or_else(|| {
        ApiError::bad_request("invalid_query", "time range is outside the supported range")
    })?;
    u64::try_from(nanos).map_err(|_| {
        ApiError::bad_request("invalid_query", "time range must not precede Unix epoch")
    })
}
