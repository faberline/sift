//! How large and how old a replicated append batch may grow.

use std::time::Duration;

pub const RAFT_BATCH_MAX_BYTES: usize = 1_048_576;

pub const RAFT_BATCH_MAX_ITEMS: usize = 1_000;

pub const RAFT_BATCH_MAX_DELAY: Duration = Duration::from_millis(10);
