//! How a retention pass pages through segments and tracks the oldest retained
//! event.

pub(in crate::archive) const RETENTION_SCAN_BATCH_SEGMENTS: usize = 64;

pub(in crate::archive) fn include_retention_oldest(oldest: &mut Option<i64>, candidate: i64) {
    *oldest = Some(
        oldest
            .map(|value| value.min(candidate))
            .unwrap_or(candidate),
    );
}
