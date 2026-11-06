//! RestoreStorePort isolates archive I/O.

use crate::archive::domain::control_values::RestoreState;
use crate::shared_kernel::layout_manifest::LayoutManifest;
use std::path::Path;

pub(crate) trait RestoreLayoutLease: Send {
    type Error;
    fn manifest(&self) -> &LayoutManifest;
    fn mark_restored_from(&mut self, manifest_uri: &str) -> Result<(), Self::Error>;
}

pub(crate) trait RestoreStorePort: Send + Sync {
    type Error;
    fn open_layout(
        &self,
        target: &Path,
    ) -> Result<Box<dyn RestoreLayoutLease<Error = Self::Error>>, Self::Error>;
    fn read_restore_state(&self, path: &Path) -> Result<Option<RestoreState>, Self::Error>;
    fn persist_restore_state(&self, path: &Path, state: &RestoreState) -> Result<(), Self::Error>;
    fn validate_building_restore_target(&self, target: &Path) -> Result<(), Self::Error>;
    fn publish_restore_stage(&self, target: &Path) -> Result<(), Self::Error>;
    fn remove_restore_stage(&self, stage: &Path) -> Result<(), Self::Error>;
    fn require_empty_restore_target(&self, target: &Path) -> Result<(), Self::Error>;
}
