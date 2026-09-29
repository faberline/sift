//! The backup module's public paths. The operations context now owns snapshot
//! backup and restore; these re-exports keep `sift::backup::*` compiling for
//! callers outside the crate.

pub use crate::operations::application::journal_backup::{backup_journal, restore_journal};
pub use crate::operations::infrastructure::live_snapshot_client::{
    backup_live_journal, backup_live_journal_authenticated, fetch_live_snapshot,
    fetch_live_snapshot_authenticated,
};
