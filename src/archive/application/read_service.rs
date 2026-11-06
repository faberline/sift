//! Archive reads share I/O services and own no journal reference.

use crate::archive::application::archive_services::ArchiveServices;
use crate::archive::application::committed_event_reader::CommittedEventReader;
use crate::shared_kernel::archive_manifest::ArchiveManifest;
use crate::shared_kernel::archive_receipts::{
    ArchiveCommitStatus, ArchiveReplay, LocalArchiveStatus, RemoteRetainedState,
};
use crate::shared_kernel::archive_watermarks::ArchiveWatermarks;
use crate::shared_kernel::dedupe_receipt::DedupeReceipt;
use crate::shared_kernel::event::SignalKind;
use crate::shared_kernel::stored_event::StoredEvent;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Arc;

pub(crate) struct ArchiveReadService {
    services: Arc<ArchiveServices>,
}

impl ArchiveReadService {
    pub(crate) fn new(services: Arc<ArchiveServices>) -> Self {
        Self { services }
    }

    pub(crate) fn committed_watermarks(&self, root: &Path) -> Result<ArchiveWatermarks> {
        crate::archive::application::archive_status_queries::committed_watermarks(
            &self.services,
            root,
        )
    }

    pub(crate) fn committed_status(&self, root: &Path) -> Result<Option<ArchiveCommitStatus>> {
        crate::archive::application::archive_status_queries::committed_status(&self.services, root)
    }

    pub(crate) fn local_committed_status(&self, root: &Path) -> Result<Option<LocalArchiveStatus>> {
        crate::archive::application::archive_status_queries::local_committed_status(
            &self.services,
            root,
        )
    }

    pub(crate) fn remote_retained_state(&self, root: &Path) -> Result<Option<RemoteRetainedState>> {
        crate::archive::application::archive_status_queries::remote_retained_state(
            &self.services,
            root,
        )
    }

    pub(crate) fn read_committed_events_after(
        &self,
        root: &Path,
        after: u64,
        limit: usize,
    ) -> Result<Option<Vec<StoredEvent>>> {
        crate::archive::application::replay_committed_events::read_committed_events_after(
            &self.services,
            root,
            after,
            limit,
        )
    }

    pub(crate) fn open_reader(
        &self,
        root: &Path,
        after: u64,
    ) -> Result<Option<CommittedEventReader>> {
        CommittedEventReader::open_with_services(&self.services, root, after)
    }

    pub(crate) fn replay_recent_events(
        &self,
        root: &Path,
        since: DateTime<Utc>,
        visitor: &mut dyn FnMut(StoredEvent) -> Result<()>,
    ) -> Result<Option<ArchiveReplay>> {
        crate::archive::application::replay_recent_committed::replay_recent_committed_events(
            &self.services,
            root,
            since,
            visitor,
        )
    }

    pub(crate) fn replay_recent_receipts(
        &self,
        root: &Path,
        since: DateTime<Utc>,
        visitor: &mut dyn FnMut(DedupeReceipt) -> Result<()>,
    ) -> Result<Option<u64>> {
        crate::archive::application::replay_recent_committed::replay_recent_committed_receipts(
            &self.services,
            root,
            since,
            visitor,
        )
    }

    pub(crate) fn validate_manifest(&self, manifest: &ArchiveManifest) -> Result<()> {
        Ok(crate::archive::application::manifest_validation::validate_archive_manifest(manifest)?)
    }

    pub(crate) fn replay_committed_events<F>(
        &self,
        root: &Path,
        signal: SignalKind,
        project: &str,
        environment: Option<&str>,
        start: Option<&str>,
        end: Option<&str>,
        visitor: F,
    ) -> Result<Option<ArchiveReplay>>
    where
        F: FnMut(StoredEvent) -> Result<()>,
    {
        crate::archive::application::replay_committed_events::replay_committed_events(
            &self.services,
            root,
            signal,
            project,
            environment,
            start,
            end,
            visitor,
        )
    }
}
