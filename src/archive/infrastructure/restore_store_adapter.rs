//! Use the existing archive I/O operations and error chains.

use crate::archive::domain::control_values::RestoreState;
use crate::archive::domain::restore_store_port::RestoreStorePort;
use anyhow::Result;
use std::path::Path;

pub(crate) struct NativeRestoreStorePort;

impl RestoreStorePort for NativeRestoreStorePort {
    type Error = anyhow::Error;
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
