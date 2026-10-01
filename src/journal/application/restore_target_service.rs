//! Cold restore uses the same journal staging and late blob writer.

use crate::journal::application::journal_service::{DurableJournal, JournalService};
use crate::journal::domain::restore_target::{RestoreBlobWriter, RestoreResources};
use crate::journal::domain::snapshot_ports::JournalStaging;
use crate::shared_kernel::event::ContentBlobRef;
use crate::shared_kernel::shard_route::EpochMap;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

pub(crate) struct RestoreTargetService {
    resources: Arc<dyn RestoreResources<Error = anyhow::Error>>,
    staging: Arc<dyn JournalStaging<Error = anyhow::Error>>,
}

pub(crate) struct RestoreBlobService {
    writer: Box<dyn RestoreBlobWriter<Error = anyhow::Error>>,
}

impl RestoreTargetService {
    pub(crate) fn new(
        resources: Arc<dyn RestoreResources<Error = anyhow::Error>>,
        staging: Arc<dyn JournalStaging<Error = anyhow::Error>>,
    ) -> Self {
        Self { resources, staging }
    }

    pub(crate) fn write_epochs(&self, root: &Path, epochs: &[EpochMap]) -> Result<()> {
        self.resources.write_epochs(root, epochs)
    }

    pub(crate) fn open_journal(&self, root: &Path) -> Result<DurableJournal> {
        self.staging.open(root).map(JournalService::owned)
    }

    pub(crate) fn open_blob_writer(
        &self,
        root: &Path,
        externalize_bytes: usize,
    ) -> Result<RestoreBlobService> {
        self.resources
            .open_blob_writer(root, externalize_bytes)
            .map(|writer| RestoreBlobService { writer })
    }
}

impl RestoreBlobService {
    pub(crate) fn put(&self, bytes: &[u8], encoding: String) -> Result<ContentBlobRef> {
        self.writer.put(bytes, encoding)
    }
}
