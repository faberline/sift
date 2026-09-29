//! Where the state machine's last checkpoint left the journal.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::journal) struct CheckpointPosition {
    pub(in crate::journal) applied_index: u64,
    pub(in crate::journal) raw_cursor: u64,
}
