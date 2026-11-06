//! The restore layout lease keeps the original lock scope.

use crate::archive::domain::restore_store_port::RestoreLayoutLease;
use crate::shared_kernel::layout_manifest::LayoutManifest;
use anyhow::Result;

pub(crate) struct RestoreLayoutService {
    lease: Box<dyn RestoreLayoutLease<Error = anyhow::Error>>,
}

impl RestoreLayoutService {
    pub(super) fn new(lease: Box<dyn RestoreLayoutLease<Error = anyhow::Error>>) -> Self {
        Self { lease }
    }
    pub(crate) fn manifest(&self) -> &LayoutManifest {
        self.lease.manifest()
    }
    pub(crate) fn mark_restored_from(&mut self, manifest_uri: &str) -> Result<()> {
        self.lease.mark_restored_from(manifest_uri)
    }
}
