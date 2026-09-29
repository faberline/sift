//! Which routes the gateway sends to the query role instead of the store role.

pub(in crate::operations) fn is_query_path(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/query"
            | "/api/v1/logs/tail"
            | "/api/v1/correlate"
            | "/api/v1/services"
            | "/prometheus/api/v1/query"
            | "/prometheus/api/v1/query_range"
    ) || path.starts_with("/api/v1/traces/")
        || path.starts_with("/api/v1/queries/")
}
