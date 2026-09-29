//! The journal's Raft boundary: the replicated command codec, the state machine
//! that applies commands to the journal, its snapshots and archive, local and
//! resident checkpoints, and the three-voter membership policy.

pub(crate) mod archive_checkpoint;
pub(crate) mod archive_checkpoint_install;
pub(crate) mod command_codec;
pub(crate) mod command_committer;
pub(crate) mod control_state_store;
pub(crate) mod diagnostics;
pub(crate) mod local_checkpoint;
pub(crate) mod raft_state_machine_impl;
pub(crate) mod resident_checkpoint;
pub(crate) mod sift_membership_policy;
pub(crate) mod sift_state_machine;
pub(crate) mod sift_state_machine_checkpoints;
pub(crate) mod snapshot_format;
pub(crate) mod snapshot_restore;
pub(crate) mod snapshot_writer;
