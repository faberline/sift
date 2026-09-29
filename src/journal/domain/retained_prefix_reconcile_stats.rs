//! What reconciling the retained segment prefix with the archive changed.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetainedPrefixReconcileStats {
    pub max_buffered_events: usize,
}
