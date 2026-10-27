//! Assembly injects one shared set of storage and archive ports.
use crate::journal::domain::{
    archive_access::{ArchiveMaintenance, ArchiveRead},
    recovery_store::RecoveryStore,
    snapshot_ports::{JournalStaging, SnapshotCodec},
};
use std::sync::Arc;
pub(crate) struct JournalDependencies {
    pub(crate) resources: Arc<dyn RecoveryStore<Error = anyhow::Error>>,
    pub(crate) archive_read: Arc<dyn ArchiveRead<Error = anyhow::Error>>,
    pub(crate) archive_maintenance: Arc<dyn ArchiveMaintenance<Error = anyhow::Error>>,
    pub(crate) snapshots: Arc<dyn SnapshotCodec<Error = anyhow::Error>>,
    pub(crate) staging: Arc<dyn JournalStaging<Error = anyhow::Error>>,
}
