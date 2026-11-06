//! The number of results a query returns when it does not set a limit.

pub(crate) const DEFAULT_QUERY_LIMIT: usize = 100;

pub(crate) const fn default_query_limit() -> usize {
    DEFAULT_QUERY_LIMIT
}
