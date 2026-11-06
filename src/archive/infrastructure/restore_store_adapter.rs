//! Use the existing archive I/O operations and error chains.

use crate::archive::domain::control_values::RestoreState;
use crate::archive::domain::restore_store_port::{RestoreLayoutLease, RestoreStorePort};
use crate::node::domain::storage_role::StorageRole;
use crate::node::infrastructure::data_layout::DataLayout;
use crate::shared_kernel::layout_manifest::LayoutManifest;
use anyhow::Result;
use std::path::Path;

pub(crate) struct NativeRestoreStorePort;

struct NativeRestoreLayout(DataLayout);

impl RestoreLayoutLease for NativeRestoreLayout {
    type Error = anyhow::Error;
    fn manifest(&self) -> &LayoutManifest {
        self.0.manifest()
    }
    fn mark_restored_from(&mut self, manifest_uri: &str) -> Result<()> {
        self.0.mark_restored_from(manifest_uri)
    }
}

impl RestoreStorePort for NativeRestoreStorePort {
    type Error = anyhow::Error;
    fn open_layout(
        &self,
        target: &Path,
    ) -> Result<Box<dyn RestoreLayoutLease<Error = Self::Error>>> {
        Ok(Box::new(NativeRestoreLayout(DataLayout::open(
            target,
            StorageRole::All,
        )?)))
    }
    fn read_restore_state(&self, path: &Path) -> Result<Option<RestoreState>> {
        crate::archive::infrastructure::restore_state::read_restore_state(path)
    }
    fn persist_restore_state(&self, path: &Path, state: &RestoreState) -> Result<()> {
        crate::archive::infrastructure::restore_state::persist_restore_state(path, state)
    }
    fn validate_building_restore_target(&self, target: &Path) -> Result<()> {
        crate::archive::infrastructure::restore_state::validate_building_restore_target(target)
    }
    fn publish_restore_stage(&self, target: &Path) -> Result<()> {
        crate::archive::infrastructure::restore_state::publish_restore_stage(target)
    }
    fn remove_restore_stage(&self, stage: &Path) -> Result<()> {
        crate::archive::infrastructure::restore_state::remove_restore_stage(stage)
    }
    fn require_empty_restore_target(&self, target: &Path) -> Result<()> {
        crate::archive::infrastructure::restore_state::require_empty_restore_target(target)
    }
}
